import unittest


class TestWebStreamClose(unittest.IsolatedAsyncioTestCase):
    async def test_close_stream_writer_ignores_broken_pipe_from_wait_closed(self) -> None:
        from cccc.ports.web.stream_close import close_stream_writer

        class Writer:
            def __init__(self) -> None:
                self.closed = False

            def close(self) -> None:
                self.closed = True

            async def wait_closed(self) -> None:
                raise BrokenPipeError("gone")

        writer = Writer()

        await close_stream_writer(writer)

        self.assertTrue(writer.closed)

    async def test_close_stream_writer_ignores_broken_pipe_from_close(self) -> None:
        from cccc.ports.web.stream_close import close_stream_writer

        class Writer:
            def close(self) -> None:
                raise BrokenPipeError("gone")

            async def wait_closed(self) -> None:
                raise AssertionError("wait_closed should not be called")

        await close_stream_writer(Writer())


if __name__ == "__main__":
    unittest.main()
