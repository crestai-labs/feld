# feld-sdk

The Python SDK of the Feld daemon API. How to install and use it is in [`docs/guide/python-sdk.md`](../../docs/guide/python-sdk.md).

## Layout

- `src/feld/api/`, `src/feld/models/`, `src/feld/client.py`, `src/feld/errors.py`, `src/feld/types.py`: generated from [`docs/guide/daemon-api.openapi.yaml`](../../docs/guide/daemon-api.openapi.yaml) by `openapi-python-client` with the template in `templates/`. Edit the definition or the template, not these files
- `src/feld/connect.py`, `events.py`, `exceptions.py`, `records.py`, `testing.py`: written by hand

## Regenerating

Run `make sdk` at the repository root after changing the OpenAPI definition, and commit the result. `make check` regenerates and fails when the committed files differ, then runs mypy and pytest. Both need [uv](https://docs.astral.sh/uv/).
