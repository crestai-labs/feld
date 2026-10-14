//! The implementations that live in a Plugin directory: every operation is the executable
//! run once with the request on stdin, and the JSON it writes is the response.

use anyhow::Result;
use serde_json::{Value, json};

use super::invoke::PluginProcess;
use super::manifest::Manifest;
use super::protocol::{
    AttachedRequest, AttachedResponse, CandidatesRequest, CandidatesResponse, ControlResponse,
    IdentityRequest, IdentityResponse, MatchRequest, MatchResponse, ObserveRequest,
    ObserveResponse, PlaceRequest, PlaceResponse, PlacementRequest, PrepareRequest,
    PrepareResponse, ProviderAttachRequest, ProviderAttachResponse, ProviderLaunchRequest,
    ProviderLaunchResponse, ResolveRequest, ResolveResponse, RestoreRequest, RestoreResponse,
    RuntimeLaunchRequest, RuntimeLaunchResponse, SelfResponse, StopRequest, TranscriptRequest,
    TranscriptResponse,
};
use super::{ExecutionProvider, RuntimeAdapter};

pub struct PluginRuntime {
    manifest: Manifest,
    process: PluginProcess,
}

impl PluginRuntime {
    pub fn new(manifest: Manifest, process: PluginProcess) -> Self {
        Self { manifest, process }
    }
}

impl RuntimeAdapter for PluginRuntime {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn launch(&self, request: &RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse> {
        self.process.call("launch", request)
    }

    fn match_process(&self, request: &MatchRequest) -> Result<MatchResponse> {
        self.process.call("match", request)
    }

    fn identity(&self, request: &IdentityRequest) -> Result<IdentityResponse> {
        self.process.call("identity", request)
    }

    fn attached(&self, request: &AttachedRequest) -> Result<AttachedResponse> {
        self.process.call("attached", request)
    }

    fn current(&self) -> Result<SelfResponse> {
        self.process.call("self", &empty())
    }

    fn prepare(&self, request: &PrepareRequest) -> Result<PrepareResponse> {
        self.process.call("prepare", request)
    }

    fn restore(&self, request: &RestoreRequest) -> Result<RestoreResponse> {
        self.process.call("restore", request)
    }

    fn transcript(&self, request: &TranscriptRequest) -> Result<TranscriptResponse> {
        self.process.call("transcript", request)
    }
}

pub struct PluginProvider {
    manifest: Manifest,
    process: PluginProcess,
}

impl PluginProvider {
    pub fn new(manifest: Manifest, process: PluginProcess) -> Self {
        Self { manifest, process }
    }
}

impl ExecutionProvider for PluginProvider {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn current(&self) -> Result<SelfResponse> {
        self.process.call("self", &empty())
    }

    fn resolve(&self, request: &ResolveRequest) -> Result<ResolveResponse> {
        self.process.call("resolve", request)
    }

    fn place(&self, request: &PlaceRequest) -> Result<PlaceResponse> {
        self.process.call("place", request)
    }

    fn launch(&self, request: &ProviderLaunchRequest) -> Result<ProviderLaunchResponse> {
        self.process.call("launch", request)
    }

    fn observe(&self, request: &ObserveRequest) -> Result<ObserveResponse> {
        self.process.call("observe", request)
    }

    fn stop(&self, request: &StopRequest) -> Result<ControlResponse> {
        self.process.call("stop", request)
    }

    fn kill(&self, request: &PlacementRequest) -> Result<ControlResponse> {
        self.process.call("kill", request)
    }

    fn cleanup(&self, request: &PlacementRequest) -> Result<()> {
        self.process
            .call_json("cleanup", &serde_json::to_value(request)?)
            .map(|_| ())
    }

    fn attach(&self, request: &ProviderAttachRequest) -> Result<ProviderAttachResponse> {
        self.process.call("attach", request)
    }

    fn candidates(&self, request: &CandidatesRequest) -> Result<CandidatesResponse> {
        self.process.call("candidates", request)
    }
}

/// The request of an operation that takes none.
fn empty() -> Value {
    json!({})
}
