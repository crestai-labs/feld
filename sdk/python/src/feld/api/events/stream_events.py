from typing import Any

from ...types import UNSET, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/events",
        "params": params,
    }

    return _kwargs
