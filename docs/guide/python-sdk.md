# Python SDK

The `feld` package calls the [daemon API](daemon-api.md) from Python. Its operations and models are generated from the [OpenAPI definition](daemon-api.openapi.yaml): answers are typed objects, times are `datetime`, and enums are `Enum`s, so a type checker such as mypy finds a misspelled key or an unchecked `None` before the program runs. The SDK has the version of Feld; use the one that matches the daemon.

## Installing

The SDK needs Python 3.11 or later. Install the version of your Feld from the repository:

```sh
pip install "feld-sdk @ git+https://github.com/crestai-labs/feld@v0.2.1#subdirectory=sdk/python"
```

The wheel is also attached to each [GitHub Release](https://github.com/crestai-labs/feld/releases) as `feld_sdk-<version>-py3-none-any.whl`.

## Connecting

```python
import feld

client = feld.connect()
```

`connect()` finds the daemon and the token the way the CLI does:

- API: the `api` argument, else `FELD_API`, else the `api` the daemon recorded in `$FELD_HOME/daemon.json`
- Token: the `token` argument, else the Session token in `FELD_SESSION_TOKEN` (inside a Session, acting as its Actor), else the local token in `$FELD_HOME/admin/api-token` (acting as Admin)
- `FELD_HOME` is `~/.feld` when unset; the `home` argument names another

A request waits for its answer without a limit; `connect(timeout=...)` sets one in seconds.

The SDK speaks HTTP to the daemon. Inside a sandbox that denies TCP (such as Codex's `workspace-write`), the CLI reaches the daemon through the [spool](../spec/design/spool.md); use the CLI there.

## Calling operations

Each operation is a module of `feld.api.<tag>`, named after its `operationId` in snake case, with `sync()` and `asyncio()`. Requests and answers are the models of `feld.models`. Path and query parameters are keyword arguments, and a request body is `body=`.

```python
from feld.api.actor import create_actor, list_actors
from feld.api.message import list_messages, send_message
from feld.api.session import get_output, get_transcript, stop_session
from feld.models import ActorKind, CreateActorBody, LaunchSpec, LaunchSpecNode, SendMessageBody

for actor in list_actors.sync(client=client, workspace="feld-dev").actors:
    if actor.current_session is not None:
        print(actor.name, actor.current_session.start)

create_actor.sync(
    client=client,
    workspace="feld-dev",
    body=CreateActorBody(
        name="nightly",
        kind=ActorKind.JOB,
        launch_spec=LaunchSpec(
            workdir="/work", provider="tmux", node=LaunchSpecNode.LOCAL, command=["make", "test"]
        ),
    ),
)

send_message.sync(
    client=client, workspace="feld-dev", actor="reviewer", body=SendMessageBody(body="Please review")
)
waiting = list_messages.sync(client=client, to="reviewer", waiting=True).messages

output = get_output.sync(client=client, session=session_id, tail=True, limit=50)  # a program Session
transcript = get_transcript.sync(client=client, session=session_id, tail=True, limit=50)  # an agent-runtime Session
stop_session.sync(client=client, session=session_id)
```

`asyncio()` takes the same arguments and is awaited. `sync_detailed()` and `asyncio_detailed()` return the status, headers, and body as they came. `GET /v1/events` is read with [`feld.events`](#following-changes).

## Errors

The errors follow the `kind` of the [CLI's errors](cli.md#common-conventions); both are `feld.FeldError`.

| Exception | When | Attributes |
|---|---|---|
| `feld.FeldRejected` (`kind` is `rejected`) | The daemon answered with an error status | `status`, `message`, and `next`, the next step when the daemon gives one |
| `feld.FeldOperational` (`kind` is `operational`) | The daemon could not be found or reached, or its answer could not be read | |

```python
try:
    stop_session.sync(client=client, session=session_id)
except feld.FeldRejected as error:
    print(error.status, error.message, error.next)
```

## Reading records

`feld.records` reads [records](../spec/concepts/glossary.md#record) in `sequence` order with the filters of `GET /v1/records`. `iterate()` reads the records after `after` until the last one there is, and `follow()` then keeps reading the ones appended, every `interval` seconds (1 by default), without end.

```python
from feld import records
from feld.models import RecordKind

for record in records.follow(client, workspace="feld-dev", kind=RecordKind.SESSION_ENDED):
    print(record.sequence, record.at, record.actor.name, record.data["endKind"])
```

To pick up where a program left off, pass the `sequence` of the last record it handled as `after`.

## Following changes

`feld.events.stream()` reads the notifications of [`GET /v1/events`](daemon-api.md#real-time-updates) as they arrive, each an `Event` with the kind of the changed resource and its ID, until the daemon closes the stream. An event carries no values; redo the read it names.

```python
from feld import events

for event in events.stream(client, workspace="feld-dev"):
    if event.resource == "message":
        print("new Message", event.id)
```

## Testing

`feld.testing.FakeDaemon` stands in for the daemon. It answers each `(method, path)` with the models registered for it, in turn, and keeps the requests it received, so a test builds its data from the same models the SDK returns.

```python
from feld.api.message import list_messages
from feld.models import MessageSummaryList
from feld.testing import FakeDaemon

def test_reads_the_mailbox() -> None:
    fake = FakeDaemon().on("GET", "/v1/messages", MessageSummaryList(messages=[]))
    assert list_messages.sync(client=fake.client(), to="dev").messages == []
    assert fake.requests[0].url.params["to"] == "dev"
```

`reject(method, path, status, message, next=None)` answers with the daemon's error, and an answer can also be a function from the `httpx.Request` to an `httpx.Response`. A route with no answer gets `404`.
