# Python SDK

The `feld` package calls the [daemon API](daemon-api.md) from Python. `feld.Client` groups the operations by resource, and its answers are typed objects generated from the [OpenAPI definition](daemon-api.openapi.yaml): times are `datetime` and enums are `Enum`s, so a type checker such as mypy finds a misspelled key or an unchecked `None` before the program runs. The SDK has the version of Feld; use the one that matches the daemon.

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

A request waits for its answer without a limit; `connect(timeout=...)` sets one in seconds. `feld.Client(base_url, token)` builds a client without the lookup.

The SDK speaks HTTP to the daemon. Inside a sandbox that denies TCP (such as Codex's `workspace-write`), the CLI reaches the daemon through the [spool](../spec/design/spool.md); use the CLI there.

## Calling operations

The client has one attribute per resource, and each operation is a method of it:

| Attribute | Methods |
|---|---|
| `workspaces` | `list`, `get`, `create`, `delete` |
| `actors` | `list`, `get`, `get_by_id`, `create`, `declare`, `update`, `delete`, `start`, `attach`, `restart`, `set_label`, `clear_label` |
| `sessions` | `list`, `get`, `stop`, `kill`, `observe`, `output`, `transcript` |
| `messages` | `list`, `send`, `get`, `delete` |
| `records` | `iterate`, `follow` ([Reading records](#reading-records)) |
| `events` | `stream` ([Following changes](#following-changes)) |
| `daemon` | `status`, `runtimes`, `providers` |

Path parameters are positional, and query parameters and request body fields are keyword arguments in snake case (`from` is `from_`). An argument left out, or `None`, is not sent; where the daemon gives `null` a meaning of its own (`actors.update(accept_from=feld.models.NULL)` removes it), `feld.models.NULL` sends it. A method returns the resource itself (an `Actor`, a `list[MessageSummary]`), or the answer's model when the answer carries more than the resource (`sessions.stop` returns a `LifecycleResponse` with `already_requested`). The types the methods return and accept are in `feld.models`.

```python
from feld.models import ActorKind, LaunchSpec, LaunchSpecNode

for actor in client.actors.list("feld-dev"):
    if actor.current_session is not None:
        print(actor.name, actor.current_session.start)

client.actors.create(
    "feld-dev",
    name="nightly",
    kind=ActorKind.JOB,
    launch_spec=LaunchSpec(
        workdir="/work", provider="tmux", node=LaunchSpecNode.LOCAL, command=["make", "test"]
    ),
)

client.messages.send("feld-dev", "reviewer", body="Please review")
waiting = client.messages.list(to="reviewer", waiting=True)

session_id = "session_..."
output = client.sessions.output(session_id, tail=True, limit=50)  # a program Session
transcript = client.sessions.transcript(session_id, tail=True, limit=50)  # an agent-runtime Session
client.sessions.stop(session_id)
```

`session.placement.placement` is a `dict` whose keys belong to the Provider.

## Errors

The errors follow the `kind` of the [CLI's errors](cli.md#common-conventions); both are `feld.FeldError`.

| Exception | When | Attributes |
|---|---|---|
| `feld.FeldRejected` (`kind` is `rejected`) | The daemon answered with an error status | `status`, `message`, and `next`, the next step when the daemon gives one |
| `feld.FeldOperational` (`kind` is `operational`) | The daemon could not be found or reached, or its answer could not be read | |

```python
try:
    client.sessions.stop(session_id)
except feld.FeldRejected as error:
    print(error.status, error.message, error.next)
```

## Reading records

`client.records` reads [records](../spec/concepts/glossary.md#record) in `sequence` order with the filters of `GET /v1/records`. `iterate()` reads the records after `after` until the last one there is, and `follow()` then keeps reading the ones appended, every `interval` seconds (1 by default), without end.

```python
from feld.models import RecordKind

for record in client.records.follow(workspace="feld-dev", kind=RecordKind.SESSION_ENDED):
    print(record.sequence, record.at, record.actor.name, record.data["endKind"])
```

To pick up where a program left off, pass the `sequence` of the last record it handled as `after`.

## Following changes

`client.events.stream()` reads the notifications of [`GET /v1/events`](daemon-api.md#real-time-updates) as they arrive, each a `feld.models.Event` with the kind of the changed resource and its ID, until the daemon closes the stream. An event carries no values; redo the read it names.

```python
for event in client.events.stream(workspace="feld-dev"):
    if event.resource == "message":
        print("new Message", event.id)
```

## Testing

`feld.testing.FakeDaemon` stands in for the daemon. It answers each `(method, path)` with the answers registered for it, in turn, and keeps the requests it received. An answer is a model of `feld.models`, a list of them, or a `dict` that is the JSON body and may hold models, so a test builds its data from the same models the SDK returns.

```python
from feld.testing import FakeDaemon

def test_reads_the_mailbox() -> None:
    fake = FakeDaemon().on("GET", "/v1/messages", {"messages": []})
    assert fake.client().messages.list(to="dev") == []
    assert fake.requests[0].url.params["to"] == "dev"
```

`reject(method, path, status, message, next=None)` answers with the daemon's error, and an answer can also be a function from the `httpx.Request` to an `httpx.Response`. `json()` is the JSON body of a received request. A route with no answer gets `404`.
