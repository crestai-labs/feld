from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.launch_spec_node import LaunchSpecNode
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.launch_spec_env import LaunchSpecEnv
    from ..models.launch_spec_provider_options import LaunchSpecProviderOptions
    from ..models.launch_spec_runtime_settings import LaunchSpecRuntimeSettings


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
            runtime_settings (LaunchSpecRuntimeSettings | Unset): Runtime settings. The Runtime Adapter defines the
                contents; the daemon stores them without reading and passes them to the Adapter at launch. `codex` takes the
                items of Codex's `config.toml`.
                Keys owned by the Adapter (`ownedSettings` in the manifest: `otelHeadersHelper` and `statusLine` for `claude-
                code`, `hooks` and `otel` for `codex`) cannot be specified.
                Requires `runtime`.
            trust_workdir (bool | Unset): Whether Admin allows settings that the workdir brings into the Runtime (such as
                Codex's `.codex/` settings and hooks) to take effect without confirmation. Defaults to false.
                Handling is defined by each Runtime Adapter. Requires `runtime`.
                Only an Admin token can set it to true; a Session token request that includes true returns 403.
            command (list[str] | Unset):
            env (LaunchSpecEnv | Unset): Environment variables added to the environment that launches the Session. Names
                consist of alphanumerics and `_` and do not start with a digit.
                Variables that Feld sets (such as `FELD_*`) take precedence over values of the same name.
            provider_options (LaunchSpecProviderOptions | Unset): Provider-specific options, built from the CLI's
                `--<provider>-<key> <value>`.
                tmux has `socket`, `unit` (`session` / `window` / `pane`), and the borrowed `session` / `window`.
                herdr has `session` (a session name or socket path), `unit` (`workspace` / `tab` / `pane`), the borrowed
                `workspace` / `pane`, and `split` (`right` / `down`) when `unit` is `pane`.
    """

    workdir: str
    provider: str
    node: LaunchSpecNode
    runtime: str | Unset = UNSET
    runtime_settings: LaunchSpecRuntimeSettings | Unset = UNSET
    trust_workdir: bool | Unset = UNSET
    command: list[str] | Unset = UNSET
    env: LaunchSpecEnv | Unset = UNSET
    provider_options: LaunchSpecProviderOptions | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        workdir = self.workdir

        provider = self.provider

        node = self.node.value

        runtime = self.runtime

        runtime_settings: dict[str, Any] | Unset = UNSET
        if not isinstance(self.runtime_settings, Unset):
            runtime_settings = self.runtime_settings.to_dict()

        trust_workdir = self.trust_workdir

        command: list[str] | Unset = UNSET
        if not isinstance(self.command, Unset):
            command = self.command

        env: dict[str, Any] | Unset = UNSET
        if not isinstance(self.env, Unset):
            env = self.env.to_dict()

        provider_options: dict[str, Any] | Unset = UNSET
        if not isinstance(self.provider_options, Unset):
            provider_options = self.provider_options.to_dict()

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
        if provider_options is not UNSET:
            field_dict["providerOptions"] = provider_options

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.launch_spec_env import LaunchSpecEnv
        from ..models.launch_spec_provider_options import LaunchSpecProviderOptions
        from ..models.launch_spec_runtime_settings import LaunchSpecRuntimeSettings

        d = dict(src_dict)
        workdir = d.pop("workdir")

        provider = d.pop("provider")

        node = LaunchSpecNode(d.pop("node"))

        runtime = d.pop("runtime", UNSET)

        _runtime_settings = d.pop("runtimeSettings", UNSET)
        runtime_settings: LaunchSpecRuntimeSettings | Unset
        if isinstance(_runtime_settings, Unset):
            runtime_settings = UNSET
        else:
            runtime_settings = LaunchSpecRuntimeSettings.from_dict(_runtime_settings)

        trust_workdir = d.pop("trustWorkdir", UNSET)

        command = cast(list[str], d.pop("command", UNSET))

        _env = d.pop("env", UNSET)
        env: LaunchSpecEnv | Unset
        if isinstance(_env, Unset):
            env = UNSET
        else:
            env = LaunchSpecEnv.from_dict(_env)

        _provider_options = d.pop("providerOptions", UNSET)
        provider_options: LaunchSpecProviderOptions | Unset
        if isinstance(_provider_options, Unset):
            provider_options = UNSET
        else:
            provider_options = LaunchSpecProviderOptions.from_dict(_provider_options)

        launch_spec = cls(
            workdir=workdir,
            provider=provider,
            node=node,
            runtime=runtime,
            runtime_settings=runtime_settings,
            trust_workdir=trust_workdir,
            command=command,
            env=env,
            provider_options=provider_options,
        )

        return launch_spec
