from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from .._types import UNSET, Unset

T = TypeVar("T", bound="LaunchSpecPatch")


@_attrs_define
class LaunchSpecPatch:
    """For each top-level field, replaces it if present, keeps it if absent, and removes it if `null`.

    Attributes:
        runtime (None | str | Unset):
        runtime_settings (dict[str, Any] | None | Unset):
        trust_workdir (bool | None | Unset):
        command (list[str] | None | Unset):
        workdir (None | str | Unset):
        env (dict[str, Any] | None | Unset): Replaced as a whole
        env_file (None | str | Unset):
        provider (None | str | Unset):
        node (None | str | Unset):
        provider_options (dict[str, Any] | None | Unset): Replaced as a whole
    """

    runtime: None | str | Unset = UNSET
    runtime_settings: dict[str, Any] | None | Unset = UNSET
    trust_workdir: bool | None | Unset = UNSET
    command: list[str] | None | Unset = UNSET
    workdir: None | str | Unset = UNSET
    env: dict[str, Any] | None | Unset = UNSET
    env_file: None | str | Unset = UNSET
    provider: None | str | Unset = UNSET
    node: None | str | Unset = UNSET
    provider_options: dict[str, Any] | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        runtime: None | str | Unset
        if isinstance(self.runtime, Unset):
            runtime = UNSET
        else:
            runtime = self.runtime

        runtime_settings: Any | None | Unset
        if isinstance(self.runtime_settings, Unset):
            runtime_settings = UNSET
        else:
            runtime_settings = self.runtime_settings

        trust_workdir: bool | None | Unset
        if isinstance(self.trust_workdir, Unset):
            trust_workdir = UNSET
        else:
            trust_workdir = self.trust_workdir

        command: list[str] | None | Unset
        if isinstance(self.command, Unset):
            command = UNSET
        elif isinstance(self.command, list):
            command = self.command

        else:
            command = self.command

        workdir: None | str | Unset
        if isinstance(self.workdir, Unset):
            workdir = UNSET
        else:
            workdir = self.workdir

        env: Any | None | Unset
        if isinstance(self.env, Unset):
            env = UNSET
        else:
            env = self.env

        env_file: None | str | Unset
        if isinstance(self.env_file, Unset):
            env_file = UNSET
        else:
            env_file = self.env_file

        provider: None | str | Unset
        if isinstance(self.provider, Unset):
            provider = UNSET
        else:
            provider = self.provider

        node: None | str | Unset
        if isinstance(self.node, Unset):
            node = UNSET
        else:
            node = self.node

        provider_options: Any | None | Unset
        if isinstance(self.provider_options, Unset):
            provider_options = UNSET
        else:
            provider_options = self.provider_options

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if runtime is not UNSET:
            field_dict["runtime"] = runtime
        if runtime_settings is not UNSET:
            field_dict["runtimeSettings"] = runtime_settings
        if trust_workdir is not UNSET:
            field_dict["trustWorkdir"] = trust_workdir
        if command is not UNSET:
            field_dict["command"] = command
        if workdir is not UNSET:
            field_dict["workdir"] = workdir
        if env is not UNSET:
            field_dict["env"] = env
        if env_file is not UNSET:
            field_dict["envFile"] = env_file
        if provider is not UNSET:
            field_dict["provider"] = provider
        if node is not UNSET:
            field_dict["node"] = node
        if provider_options is not UNSET:
            field_dict["providerOptions"] = provider_options

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime = _parse_runtime(d.pop("runtime", UNSET))

        def _parse_runtime_settings(data: object) -> Any | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Any | None | Unset, data)

        runtime_settings = _parse_runtime_settings(d.pop("runtimeSettings", UNSET))

        def _parse_trust_workdir(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        trust_workdir = _parse_trust_workdir(d.pop("trustWorkdir", UNSET))

        def _parse_command(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                command_type_0 = cast(list[str], data)

                return command_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        command = _parse_command(d.pop("command", UNSET))

        def _parse_workdir(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        workdir = _parse_workdir(d.pop("workdir", UNSET))

        def _parse_env(data: object) -> Any | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Any | None | Unset, data)

        env = _parse_env(d.pop("env", UNSET))

        def _parse_env_file(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        env_file = _parse_env_file(d.pop("envFile", UNSET))

        def _parse_provider(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        provider = _parse_provider(d.pop("provider", UNSET))

        def _parse_node(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        node = _parse_node(d.pop("node", UNSET))

        def _parse_provider_options(data: object) -> Any | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(Any | None | Unset, data)

        provider_options = _parse_provider_options(d.pop("providerOptions", UNSET))

        launch_spec_patch = cls(
            runtime=runtime,
            runtime_settings=runtime_settings,
            trust_workdir=trust_workdir,
            command=command,
            workdir=workdir,
            env=env,
            env_file=env_file,
            provider=provider,
            node=node,
            provider_options=provider_options,
        )

        return launch_spec_patch
