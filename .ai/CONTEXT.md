# AI Context Route

```yaml
schema_version: 1
context_repo: https://github.com/madebycli/master-context
project_id: gif-player
source_repo: https://github.com/madebycli/GIF-Player
context_root: projects/gif-player/
entrypoint: projects/gif-player/INDEX.md
```

## Mandatory AI behavior

Use this exact project route. Validate it against `REGISTRY.yaml`, read the declared entrypoint first, never scan sibling project folders, follow only task-relevant graph links, reconcile durable context before declaring work complete, and archive reusable prompts/plans/handoffs under `prompts/gif-player/` when possible.
