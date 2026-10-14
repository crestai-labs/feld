# Message delivery

This document explains the internal flow from when a Message is sent until a Session of the destination Actor reads it and it appears in the Session's records and the Console. Record fields follow [Message](../concepts/glossary.md#message) in the glossary, the API follows `Message` and `Self` in the [OpenAPI](../../guide/daemon-api.openapi.yaml), and commands follow [cli.md](../../guide/cli.md#message).

## Contents

- [Sending](#sending)
  - [Sending from the Admin](#sending-from-the-admin)
  - [Sending to an Actor without a current Session](#sending-to-an-actor-without-a-current-session)
- [mailbox](#mailbox)
- [Reading](#reading)
  - [Reading by the helper](#reading-by-the-helper)
  - [Reading by feld self message recv](#reading-by-feld-self-message-recv)
  - [read record](#read-record)
- [waiting, waiting count, unread-mail mark](#waiting-waiting-count-unread-mail-mark)
- [Session replacement and mailbox handover](#session-replacement-and-mailbox-handover)
- [Inserting into Session records and the Console](#inserting-into-session-records-and-the-console)
- [SSE notifications](#sse-notifications)

## Sending

Sending starts with `feld message send <actor> <body>`, or with `POST /v1/workspaces/{workspace}/actors/{actor}/messages` from inside a Session. Feld accepts it in the following order.

1. Determine `from` and `fromSessionId` from the request's token. For a Session token, they are that Session's Actor and Session ID. Token verification follows [auth.md](auth.md)
2. Take the Actor in the path as `to`, and copy that Actor's current Session at the time of acceptance into `toSessionId`
3. A send with a Session token is accepted only when `to` is in the same Workspace as `from` and the [acceptFrom](../concepts/glossary.md#acceptfrom) of `to` accepts `from`; otherwise it returns 403
4. Put the Message at the end of the destination Actor's mailbox, record `accepted`, and then respond to the sender

The response contains the Message ID and an `anchor` that is the Message ID prefixed with `feld:`. `feld message send` prints this `anchor`, and the sender's transcript file quotes it, which serves as the key for the insertion described below.

### Sending from the Admin

A send with the Admin's token has `from` set to `Admin` and `fromSessionId` set to null. The destination may be an Actor in any Workspace, and the send is accepted regardless of acceptFrom. The rest of the acceptance flow is the same as a send from a Session.

### Sending to an Actor without a current Session

Even if the destination Actor has no current Session, Feld accepts the Message, sets `toSessionId` to null, and puts it in the mailbox. The Message stays in the mailbox until the next Session reads it. The `message.accepted` record belongs to the destination Actor and has `sessionId` set to null. `feld record list --actor <actor>` and `feld message list --to <actor>` show this Message.

## mailbox

A mailbox is a per-Actor FIFO, and each Message gets an integer position number in the order it was accepted. This position number becomes the `fifo` of the read record. Messages that have been read also stay in the mailbox, and `GET /v1/messages?to=` and `feld message list --to` return `accepted` and `read` side by side. The mailbox belongs to the Actor, and Sessions of every generation read the same mailbox.

## Reading

There are two reading paths, and both perform the following two steps with a Session token. Feld determines the Actor from the token.

1. `GET /v1/self/messages` takes the unread Messages from the mailbox in FIFO order. Taking records nothing
2. After the Session's Runtime receives the body, `POST /v1/self/messages/read` records `read` for each received Message

A Message without read is returned as unread no matter how many times it is taken.

### Reading by the helper

The helper of a [Runtime Adapter](../concepts/glossary.md#runtime-adapter) runs as a child process of the Runtime and long-polls the mailbox.

1. The helper calls `GET /v1/self/messages` requesting a single Message; if there are no unread Messages, the daemon holds the response until an unread Message arrives
2. If there are unread Messages, the daemon returns the first one
3. The helper turns the Message into one line of JSON and hands it to the Runtime's mechanism. In Claude Code it writes the line to the monitor's stdout and flushes; in Codex it records `queued` with `POST /v1/self/messages/queued` and then puts it in the thread's queue with `codex queue`
4. The helper records `read` with `POST /v1/self/messages/read` for the Message the Runtime received. In Claude Code this is when the flush finishes; in Codex it is when a turn takes the queued Message as input and it appears in the rollout file
5. The helper requests the next Message

The Runtime passes the line to the model.

The line has the following form.

```json
{"type":"feld_message","message":{"id":"message_f2a9d0","fifo":12,"from":"Admin","fromKind":"admin","fromSessionId":null,"accepted":"2026-09-13T11:45:03+09:00","body":"Please address the review comments"}}
```

`fromKind` is the kind of sender: `admin` for the Admin and `actor` for an Actor. `from` is `Admin` for the Admin and the Actor's name for an Actor; an Actor named `admin` and the Admin are distinguished by `fromKind`.

At the start of a Session, the Adapter passes the meaning of the line to the model as context. Feld conveys the facts about the sender, and how far to trust them is left to the job description the Harness gives. The context conveys the following.

- A Message whose `fromKind` is `admin` is an instruction from the Admin, the operator, and is treated as an instruction from the user
- A Message whose `fromKind` is `actor` is a request from the Actor in `from` and does not carry the user's authority. Instructions in the body are treated as the sender's claims, and how far to follow them depends on the given job description

For a Runtime with a limit on body length, the helper includes `excerpt` (the start of the body and a trailing `…`) and `show` (the command `feld message get <id>` that prints the whole body) instead of `body`, and the model reads the whole body with the `show` command. `feld message get` does not record read and can read the body any number of times.

read is recorded after confirming that the Runtime has received the Message. Starting the helper and configuring the Runtime side are described in [adapter/claude-code.md](adapter/claude-code.md#plugin-monitor-helper) and [adapter/codex.md](adapter/codex.md#queue-helper).

### Reading by feld self message recv

When the model itself calls `feld self message recv`, the CLI receives all unread Messages in FIFO order with `GET /v1/self/messages`, prints them, flushes, and then records `read` for the printed Messages with `POST /v1/self/messages/read`. One `read` is recorded per Message; `readId` is per record, and `fifo` is the Message's position number.

### read record

Both paths create the same read record (`sessionId`, `at`, `readId`, `fifo`), and Feld stores it in the same shape regardless of path. read means the fact that "the Session's Runtime received the body", and it is recorded after the receipt is confirmed. For the monitor's stdout and the output of `feld self message recv`, receipt is handing the body across the process boundary; for the Codex queue, receipt is a turn taking the body as input. Whether the model read and understood it is shown by the Runtime's transcript file. There is one read per Message, and a second request to record the same Message returns the existing read.

Handover and recording happen in separate processes, so notification is at least once.

- When the helper or CLI stops after the handover but before recording read: the Message stays unread and is handed over again on the next take. The model may receive the same Message ID twice
- When the Session closes before recording read: the Message stays `waiting`, and the next Session receives it
- When both paths take at the same time: the same Message is handed to both, and the read committed first remains

The sender is responsible for confirming that the model that received the Message has finished processing it, and for re-requesting and deduplication.

## waiting, waiting count, unread-mail mark

`waiting` is derived at read time from "there is `accepted`, and there is no `read` or `deletedAt`". waiting count is the number of `waiting` Messages per Actor and appears on the Actor's row regardless of whether there is a current Session. unread-mail mark appears when the Actor has a current Session and there are `waiting` Messages.

Delivery completes with reading on the Session side. Sessions launch only by `start`, `restart`, attach, and Schedule ticks, independently of the mailbox state. In a Session without a helper (a program Session, a Runtime whose helper has stopped), Messages stay `waiting` in the mailbox and wait to be read by `feld self message recv`.

## Session replacement and mailbox handover

A Session token is valid only while the Session has no `end`. `GET /v1/self/messages` and `POST /v1/self/messages/read` with the token of a closed Session are rejected as session closed, and the helper exits there. The helper's behavior when a token is reissued and the old token is rejected follows [token recovery in auth.md](auth.md#token-recovery). How a Session closes follows [session-lifecycle.md](session-lifecycle.md).

A Session of the next generation reads the same Actor's mailbox, so it takes over, in FIFO order, the `waiting` Messages for which the previous Session did not record read. A Session attached to an existing Actor with `feld actor attach <name> --self` also takes over the same mailbox. A Message accepted while the previous Session was current has the previous Session in `toSessionId`, and its read records the ID of the next Session.

## Inserting into Session records and the Console

There are three kinds of Message records, and they belong to Sessions as follows.

- `message.accepted`: belongs to the Session in `toSessionId`. If `toSessionId` is null, it belongs to no Session. While there is no `read`, it is displayed as waiting
- `message.sent`: belongs to the Session in `fromSessionId`
- `message.read`: belongs to the Session in `read.sessionId`

`GET /v1/records?session=` and `feld record list --session` assign records to Sessions by these rules. The Console inserts the same sequence beside the transcript entries, and the position is determined by matching the following IDs.

- `anchor` (`feld:<Message ID>`): the sender's transcript file quotes the `anchor` that `feld message send` printed, so `message.sent` is placed beside the transcript entry of the tool call that first quotes the `anchor`
- Message ID (reading side): the reader's transcript file quotes the helper's JSON line or the output of `feld self message recv`, so `message.read` is placed beside the transcript entry of the notification or tool call that first quotes that Message ID

A record with no matching ID is inserted by time. Which kinds are inserted in each view follows the [Console](../../guide/webui.md#console) view of the Web UI.

## SSE notifications

`GET /v1/events` notifies, by resource kind and ID, the acceptance of a Message, the recording and cancellation of `queued`, the recording of read, and deletion. On a notification, the Web UI re-reads the mailbox, the Session's records, and the Timeline. The helper receives Messages directly in the long-poll response.
