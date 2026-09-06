# R2 retention

The stable channel retains the five newest semantic versions. Additional
operational rollback versions can be added to `config/r2-retention.json` as
explicit pins. A missing pin, malformed policy, incomplete retained release, or
inconsistent catalog makes retention fail closed.

Deletion is deliberately two-phase. The first run removes obsolete releases
from `catalog-release.json`, removes their immutable version manifests, and
writes durable markers below `os/.retention/release/`. Payloads remain available
until the manifest's advertised cache lifetime has elapsed, followed by a
24-hour grace period for active downloads. A later hourly run deletes the image,
RAUC bundle, and SBOM objects and then removes the marker.

`R2 Stable Retention` runs hourly under the shared R2 object lease. Scheduled
runs apply the policy; manual runs are dry by default. Set the manual `apply`
input only after reviewing its dry-run output. R2 lifecycle rules should remain
limited to incomplete multipart uploads because lifecycle expiration cannot
understand release aliases, manifests, catalogs, or pinned versions.
