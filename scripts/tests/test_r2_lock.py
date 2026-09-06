from __future__ import annotations

import io
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from r2_lock import LockLost, LockTimeout, acquire_lock, release_lock, renew_lock  # noqa: E402


class FakeClientError(Exception):
    def __init__(self, code: str):
        super().__init__(code)
        self.response = {"Error": {"Code": code}}


class FakeS3:
    def __init__(self) -> None:
        self.value: bytes | None = None
        self.etag = 0

    def put_object(self, *, Body: bytes, IfNoneMatch=None, IfMatch=None, **_kwargs):
        if IfNoneMatch == "*" and self.value is not None:
            raise FakeClientError("PreconditionFailed")
        if IfMatch is not None and IfMatch != f'"{self.etag}"':
            raise FakeClientError("PreconditionFailed")
        self.value = Body
        self.etag += 1

    def get_object(self, **_kwargs):
        if self.value is None:
            raise FakeClientError("NoSuchKey")
        return {"Body": io.BytesIO(self.value), "ETag": f'"{self.etag}"'}


class R2LockTests(unittest.TestCase):
    def acquire(self, client: FakeS3, owner: str, now: float = 100.0) -> None:
        acquire_lock(
            client,
            "bucket",
            "lock",
            owner,
            ttl_seconds=60,
            wait_seconds=0,
            poll_seconds=1,
            clock=lambda: now,
            sleep=lambda _seconds: None,
        )

    def test_acquires_empty_lock_and_owner_can_release(self) -> None:
        client = FakeS3()
        self.acquire(client, "run-a")
        self.assertEqual(json.loads(client.value or b"{}")['owner'], "run-a")
        self.assertTrue(
            release_lock(client, "bucket", "lock", "run-a", clock=lambda: 101.0)
        )
        self.assertEqual(
            json.loads(client.value or b"{}")["owner"], "released:run-a"
        )

    def test_live_lease_times_out_and_cannot_be_released_by_other_owner(self) -> None:
        client = FakeS3()
        self.acquire(client, "run-a")
        with self.assertRaises(LockTimeout):
            self.acquire(client, "run-b", now=101.0)
        self.assertFalse(
            release_lock(client, "bucket", "lock", "run-b", clock=lambda: 101.0)
        )
        self.assertIsNotNone(client.value)

    def test_expired_lease_is_replaced_atomically(self) -> None:
        client = FakeS3()
        self.acquire(client, "run-a")
        self.acquire(client, "run-b", now=161.0)
        self.assertEqual(json.loads(client.value or b"{}")['owner'], "run-b")
        self.assertFalse(
            release_lock(client, "bucket", "lock", "run-a", clock=lambda: 162.0)
        )
        self.assertTrue(
            release_lock(client, "bucket", "lock", "run-b", clock=lambda: 162.0)
        )

    def test_owner_can_renew_but_an_old_owner_cannot(self) -> None:
        client = FakeS3()
        self.acquire(client, "run-a")
        renew_lock(
            client,
            "bucket",
            "lock",
            "run-a",
            ttl_seconds=60,
            clock=lambda: 150.0,
        )
        lease = json.loads(client.value or b"{}")
        self.assertEqual(lease["expires_at_epoch"], 210.0)

        self.acquire(client, "run-b", now=211.0)
        with self.assertRaises(LockLost):
            renew_lock(
                client,
                "bucket",
                "lock",
                "run-a",
                ttl_seconds=60,
                clock=lambda: 212.0,
            )


if __name__ == "__main__":
    unittest.main()
