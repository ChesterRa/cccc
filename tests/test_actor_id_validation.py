import unittest


class TestActorIdValidation(unittest.TestCase):
    def test_validate_actor_id_rejects_leading_underscore(self) -> None:
        from cccc.kernel.actors import validate_actor_id

        with self.assertRaises(ValueError):
            validate_actor_id("_peer")

    def test_validate_actor_id_accepts_cjk(self) -> None:
        from cccc.kernel.actors import validate_actor_id

        aid = validate_actor_id("大将_1")
        self.assertEqual(aid, "大将_1")


if __name__ == "__main__":
    unittest.main()
