from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from .._models.launch_spec_node import LaunchSpecNode
from .._types import UNSET, Unset

T = TypeVar("T", bound="LaunchSpec")


@_attrs_define
class LaunchSpec:
    """Has exactly one of `runtime` and `command`. `runtime` is the name of an installed Runtime Adapter, and `provider`
    the name of an installed Execution Provider; an unknown name is rejected with 400.
    `workdir` must be an absolute path, and `node` must be `local`.
    `providerOptions` is stored with omitted values filled by the Provider's `resolve`; options the Provider rejects
    return 400.

        Attributes:
            workdir (str):
            provider (str):
            node (LaunchSpecNode):
            runtime (str | Unset):
            runtime_settings (dict[str, Any] | Unset): Runtime settings. The Runtime Adapter defines the contents; the
                daemon stores them without reading and passes them to the Adapter at launch. `codex` takes the items of Codex's
                `config.toml`.
                Keys owned by the Adapter (`ownedSettings` in the manifest: `otelHeadersHelper` and `statusLine` for `claude-
                code`, `hooks` and `otel` for `codex`) cannot be specified.
                Requires `runtime`.
            trust_workdir (bool | Unset): Whether Admin allows settings that the workdir brings into the Runtime (such as
                Codex's `.codex/` settings and hooks) to take effect without confirmation. Defaults to false.
                Handling is defined by each Runtime Adapter. Requires `runtime`.
                Only an Admin token can set it to true; a Session token request that includes true returns 403.
            command (list[str] | Unset):
            env (dict[str, Any] | Unset): Environment variables added to the environment that launches the Session. Names
                consist of alphanumerics and `_` and do not start with a digit.
                Variables that Feld sets (such as `FELD_*`) take precedence over values of the same name.
            env_file (str | Unset): Absolute path of a file of `KEY=VALUE` lines. The daemon reads it at each launch, and
                its variables go beneath `env`.
                Only the path is stored. A file that cannot be read or parsed fails that launch.
            provider_options (dict[str, Any] | Unset): Options of the Execution Provider, from the CLI's `--<provider>-<key>
                <value>`.
    """

    workdir: str
    provider: str
    node: LaunchSpecNode
    runtime: str | Unset = UNSET
    runtime_settings: dict[str, Any] | Unset = UNSET
    trust_workdir: bool | Unset = UNSET
    command: list[str] | Unset = UNSET
    env: dict[str, Any] | Unset = UNSET
    env_file: str | Unset = UNSET
    provider_options: dict[str, Any] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        workdir = self.workdir

        provider = self.provider

        node = self.node.value

        runtime = self.runtime

        runtime_settings = self.runtime_settings

        trust_workdir = self.trust_workdir

        command: list[str] | Unset = UNSET
        if not isinstance(self.command, Unset):
            command = self.command

        env = self.env

        env_file = self.env_file

        provider_options = self.provider_options

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workdir": workdir,
                "provider": provider,
                "node": node,
            }
        )
        if runtime is not UNSET:
            field_dict["runtime"] = runtime
        if runtime_settings is not UNSET:
            field_dict["runtimeSettings"] = runtime_settings
        if trust_workdir is not UNSET:
            field_dict["trustWorkdir"] = trust_workdir
        if command is not UNSET:
            field_dict["command"] = command
        if env is not UNSET:
            field_dict["env"] = env
        if env_file is not UNSET:
            field_dict["envFile"] = env_file
        if provider_options is not UNSET:
            field_dict["providerOptions"] = provider_options

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workdir = d.pop("workdir")

        provider = d.pop("provider")

        node = LaunchSpecNode(d.pop("node"))

        runtime = d.pop("runtime", UNSET)

        runtime_settings = d.pop("runtimeSettings", UNSET)

        trust_workdir = d.pop("trustWorkdir", UNSET)

        command = cast(list[str], d.pop("command", UNSET))

        env = d.pop("env", UNSET)

        env_file = d.pop("envFile", UNSET)

        provider_options = d.pop("providerOptions", UNSET)

        launch_spec = cls(
            workdir=workdir,
            provider=provider,
            node=node,
            runtime=runtime,
            runtime_settings=runtime_settings,
            trust_workdir=trust_workdir,
            command=command,
            env=env,
            env_file=env_file,
            provider_options=provider_options,
        )

        return launch_spec
