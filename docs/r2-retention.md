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

Immediately before retention, the workflow verifies its lock owner and renews
the shared lease for two hours. The entire renewal/retention step has a 30-minute
timeout, leaving ample lease lifetime for runner termination and cleanup. A
failed renewal prevents retention from starting; the final lock release runs
even after failure. Never increase this timeout to approach or exceed the lease
lifetime without adding an ownership-preserving renewal mechanism.

The reported retirement candidate size is not reclaimed storage. Count only
confirmed payload deletions after the cache lifetime and download grace period.
