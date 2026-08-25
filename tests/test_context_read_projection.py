import unittest


class TestContextReadProjection(unittest.TestCase):
    def test_empty_summary_derives_both_versions_from_one_state_read(self) -> None:
        from cccc.daemon.context.context_read_projection import build_empty_summary

        class RacingStorage:
            def __init__(self) -> None:
                self.reads = 0

            def load_version_state(self) -> dict:
                self.reads += 1
                if self.reads == 1:
                    return {"global_rev": 3, "tasks_rev": 2}
                return {"global_rev": 4, "tasks_rev": 3}

        storage = RacingStorage()
        result = build_empty_summary(
            storage,
            lambda _brief: {},
            lambda _tasks, **_kwargs: {},
        )

        self.assertEqual(result["version"], "ctxv:3")
        self.assertEqual(result["tasks_version"], "tasksv:2")
        self.assertEqual(storage.reads, 1)


if __name__ == "__main__":
    unittest.main()
