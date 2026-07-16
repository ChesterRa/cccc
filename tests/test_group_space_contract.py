from __future__ import annotations

import unittest

from pydantic import ValidationError


class TestGroupSpaceContract(unittest.TestCase):
    def test_provider_state_defaults(self) -> None:
        from cccc.contracts.v1.group_space import SpaceProviderState

        doc = SpaceProviderState()
        self.assertEqual(doc.provider, "notebooklm")
        self.assertFalse(doc.enabled)
        self.assertEqual(doc.mode, "disabled")

    def test_space_job_defaults(self) -> None:
        from cccc.contracts.v1.group_space import SpaceJob

        job = SpaceJob(job_id="spj_1", group_id="g_1")
        self.assertEqual(job.kind, "context_sync")
        self.assertEqual(job.state, "pending")
        self.assertEqual(job.max_attempts, 3)
        self.assertEqual(job.result, {})

    def test_space_job_state_validation(self) -> None:
        from cccc.contracts.v1.group_space import SpaceJob

        with self.assertRaises(ValidationError):
            SpaceJob(job_id="spj_1", group_id="g_1", state="unknown")

    def test_provider_credential_state_defaults(self) -> None:
        from cccc.contracts.v1.group_space import SpaceProviderCredentialState

        doc = SpaceProviderCredentialState()
        self.assertEqual(doc.provider, "notebooklm")
        self.assertEqual(doc.source, "none")
        self.assertFalse(doc.configured)


if __name__ == "__main__":
    unittest.main()
