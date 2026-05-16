from __future__ import annotations

import unittest

from pydantic import ValidationError

from cccc.ports.web.schemas import ActorRestartRequest


class TestWebActorRestartSchema(unittest.TestCase):
    def test_restart_request_rejects_legacy_session_field(self) -> None:
        with self.assertRaises(ValidationError):
            ActorRestartRequest.model_validate({"clear" + "_session": True})

    def test_restart_request_rejects_removed_session_control_field(self) -> None:
        with self.assertRaises(ValidationError):
            ActorRestartRequest.model_validate({"fresh" + "_session": True})
