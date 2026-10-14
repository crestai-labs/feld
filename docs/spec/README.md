# Spec

The specification Feld satisfies. The reading order and precedence are as follows; where a document conflicts with a higher-ranked one, the higher-ranked one wins. The surfaces through which people and programs touch Feld (CLI, daemon API, Web UI) are described in [`../guide/`](../guide/).

1. [`concepts/`](concepts/): Feld's concepts and terms.
2. [`design/`](design/): Plugin, Session lifecycle, Provider, Adapter, Message delivery, metrics, and how the Web UI reads. Implementation details.
