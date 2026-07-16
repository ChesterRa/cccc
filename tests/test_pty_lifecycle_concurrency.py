import importlib
import os
import threading
import unittest
from pathlib import Path
from unittest.mock import patch


class _SessionState:
    def __init__(self, *, group_id: str, actor_id: str) -> None:
        self.group_id = group_id
        self.actor_id = actor_id
        self.running = True
        self.stop_calls = 0

    def is_running(self) -> bool:
        return self.running

    def stop(self) -> None:
        self.stop_calls += 1
        self.running = False

    def _backlog_snapshot(self):
        return b"", 0, 0

    def returncode(self) -> int:
        return 0


class TestPtyLifecycleConcurrency(unittest.TestCase):
    @staticmethod
    def _backends():
        names = ["cccc.runners.pty_win"]
        if os.name != "nt":
            names.insert(0, "cccc.runners.pty")
        return [(name.rsplit(".", 1)[-1], importlib.import_module(name)) for name in names]

    @staticmethod
    def _start_for(supervisor, *, group_id: str = "g1", actor_id: str = "a1"):
        return supervisor.start_actor(
            group_id=group_id,
            actor_id=actor_id,
            cwd=Path.cwd(),
            command=["unused"],
            env={},
        )

    @staticmethod
    def _start(supervisor):
        return TestPtyLifecycleConcurrency._start_for(supervisor)

    def test_concurrent_starts_construct_only_one_session(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                created = []
                created_lock = threading.Lock()
                second_constructed = threading.Event()

                class Session(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        with created_lock:
                            created.append(self)
                            index = len(created)
                        if index == 1:
                            second_constructed.wait(timeout=0.25)
                        else:
                            second_constructed.set()

                results = []
                errors = []
                start_together = threading.Barrier(3)

                def start_actor() -> None:
                    start_together.wait()
                    try:
                        results.append(self._start(supervisor))
                    except BaseException as exc:
                        errors.append(exc)

                with patch.object(module, "PtySession", Session):
                    threads = [threading.Thread(target=start_actor) for _ in range(2)]
                    for thread in threads:
                        thread.start()
                    start_together.wait()
                    for thread in threads:
                        thread.join(timeout=2.0)

                self.assertFalse(errors, errors)
                self.assertTrue(all(not thread.is_alive() for thread in threads))
                self.assertEqual(len(created), 1)
                self.assertEqual(len(results), 2)
                self.assertIs(results[0], results[1])

    def test_constructor_base_exception_releases_exit_callback(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                exit_threads = []

                class Session(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        thread = threading.Thread(target=kwargs["on_exit"], args=(self,), daemon=True)
                        exit_threads.append(thread)
                        thread.start()
                        raise KeyboardInterrupt("construction interrupted")

                with patch.object(module, "PtySession", Session):
                    with self.assertRaisesRegex(KeyboardInterrupt, "construction interrupted"):
                        self._start(supervisor)

                self.assertEqual(len(exit_threads), 1)
                exit_threads[0].join(timeout=1.0)
                self.assertFalse(exit_threads[0].is_alive())

    def test_start_waits_for_stop_actor_then_constructs_replacement(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class OldSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                old = OldSession(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[("g1", "a1")] = old

                created = []

                class NewSession(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        created.append(self)

                result = []
                start_done = threading.Event()
                stop_thread = threading.Thread(
                    target=lambda: supervisor.stop_actor(group_id="g1", actor_id="a1")
                )

                def start_actor() -> None:
                    result.append(self._start(supervisor))
                    start_done.set()

                with patch.object(module, "PtySession", NewSession):
                    stop_thread.start()
                    self.assertTrue(stop_entered.wait(timeout=1.0))
                    start_thread = threading.Thread(target=start_actor)
                    start_thread.start()
                    completed_before_stop = start_done.wait(timeout=0.1)
                    release_stop.set()
                    stop_thread.join(timeout=2.0)
                    start_thread.join(timeout=2.0)

                self.assertFalse(completed_before_stop)
                self.assertFalse(stop_thread.is_alive())
                self.assertFalse(start_thread.is_alive())
                self.assertEqual(old.stop_calls, 1)
                self.assertEqual(len(created), 1)
                self.assertEqual(result, created)

    def test_stop_actor_waits_for_construction_then_stops_new_session(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                construction_entered = threading.Event()
                release_construction = threading.Event()
                created = []

                class Session(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        created.append(self)
                        construction_entered.set()
                        release_construction.wait(timeout=2.0)

                stop_done = threading.Event()

                def stop_actor() -> None:
                    supervisor.stop_actor(group_id="g1", actor_id="a1")
                    stop_done.set()

                with patch.object(module, "PtySession", Session):
                    start_thread = threading.Thread(target=lambda: self._start(supervisor))
                    start_thread.start()
                    self.assertTrue(construction_entered.wait(timeout=1.0))
                    stop_thread = threading.Thread(target=stop_actor)
                    stop_thread.start()
                    completed_during_construction = stop_done.wait(timeout=0.1)
                    release_construction.set()
                    start_thread.join(timeout=2.0)
                    stop_thread.join(timeout=2.0)

                self.assertFalse(completed_during_construction)
                self.assertFalse(start_thread.is_alive())
                self.assertFalse(stop_thread.is_alive())
                self.assertEqual(len(created), 1)
                self.assertEqual(created[0].stop_calls, 1)
                self.assertFalse(supervisor.actor_running("g1", "a1"))

    def test_concurrent_stops_call_session_stop_once(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()
                calls_lock = threading.Lock()

                class Session(_SessionState):
                    def stop(self) -> None:
                        with calls_lock:
                            self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                session = Session(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[("g1", "a1")] = session

                threads = [
                    threading.Thread(target=lambda: supervisor.stop_actor(group_id="g1", actor_id="a1"))
                    for _ in range(2)
                ]
                threads[0].start()
                self.assertTrue(stop_entered.wait(timeout=1.0))
                threads[1].start()
                threading.Event().wait(timeout=0.1)
                release_stop.set()
                for thread in threads:
                    thread.join(timeout=2.0)

                self.assertTrue(all(not thread.is_alive() for thread in threads))
                self.assertEqual(session.stop_calls, 1)

    def test_stop_actor_allows_same_key_stop_reentry_from_exit_path(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()

                class Session(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        supervisor.stop_actor(group_id=self.group_id, actor_id=self.actor_id)
                        self.running = False

                session = Session(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[("g1", "a1")] = session

                stop_thread = threading.Thread(
                    target=lambda: supervisor.stop_actor(group_id="g1", actor_id="a1"),
                    daemon=True,
                )
                stop_thread.start()
                stop_thread.join(timeout=0.25)

                self.assertFalse(stop_thread.is_alive(), "same-key stop reentry deadlocked")
                self.assertEqual(session.stop_calls, 1)

    def test_bulk_stop_allows_covered_stop_reentry_from_exit_path(self) -> None:
        for backend, module in self._backends():
            for method_name in ("stop_group", "stop_all"):
                with self.subTest(backend=backend, method=method_name):
                    supervisor = module.PtySupervisor()

                    class Session(_SessionState):
                        def stop(self) -> None:
                            self.stop_calls += 1
                            supervisor.stop_actor(group_id=self.group_id, actor_id=self.actor_id)
                            self.running = False

                    session = Session(group_id="g1", actor_id="a1")
                    with supervisor._lock:
                        supervisor._sessions[("g1", "a1")] = session

                    def bulk_stop() -> None:
                        if method_name == "stop_group":
                            supervisor.stop_group(group_id="g1")
                        else:
                            supervisor.stop_all()

                    stop_thread = threading.Thread(target=bulk_stop, daemon=True)
                    stop_thread.start()
                    stop_thread.join(timeout=0.25)

                    self.assertFalse(stop_thread.is_alive(), "bulk stop reentry deadlocked")
                    self.assertEqual(session.stop_calls, 1)

    def test_bulk_stop_waits_for_in_progress_construction(self) -> None:
        for backend, module in self._backends():
            for method_name in ("stop_group", "stop_all"):
                with self.subTest(backend=backend, method=method_name):
                    supervisor = module.PtySupervisor()
                    construction_entered = threading.Event()
                    release_construction = threading.Event()
                    created = []

                    class Session(_SessionState):
                        def __init__(self, **kwargs) -> None:
                            super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                            created.append(self)
                            construction_entered.set()
                            release_construction.wait(timeout=2.0)

                    stop_done = threading.Event()

                    def bulk_stop() -> None:
                        if method_name == "stop_group":
                            supervisor.stop_group(group_id="g1")
                        else:
                            supervisor.stop_all()
                        stop_done.set()

                    with patch.object(module, "PtySession", Session):
                        start_thread = threading.Thread(target=lambda: self._start(supervisor))
                        start_thread.start()
                        self.assertTrue(construction_entered.wait(timeout=1.0))
                        stop_thread = threading.Thread(target=bulk_stop)
                        stop_thread.start()
                        completed_during_construction = stop_done.wait(timeout=0.1)
                        release_construction.set()
                        start_thread.join(timeout=2.0)
                        stop_thread.join(timeout=2.0)

                    self.assertFalse(completed_during_construction)
                    self.assertFalse(start_thread.is_alive())
                    self.assertFalse(stop_thread.is_alive())
                    self.assertEqual(len(created), 1)
                    self.assertEqual(created[0].stop_calls, 1)

    def test_start_waits_for_bulk_stop_then_constructs_replacement(self) -> None:
        for backend, module in self._backends():
            for method_name in ("stop_group", "stop_all"):
                with self.subTest(backend=backend, method=method_name):
                    supervisor = module.PtySupervisor()
                    stop_entered = threading.Event()
                    release_stop = threading.Event()

                    class OldSession(_SessionState):
                        def stop(self) -> None:
                            self.stop_calls += 1
                            stop_entered.set()
                            release_stop.wait(timeout=2.0)
                            self.running = False

                    old = OldSession(group_id="g1", actor_id="a1")
                    with supervisor._lock:
                        supervisor._sessions[("g1", "a1")] = old

                    created = []

                    class NewSession(_SessionState):
                        def __init__(self, **kwargs) -> None:
                            super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                            created.append(self)

                    def bulk_stop() -> None:
                        if method_name == "stop_group":
                            supervisor.stop_group(group_id="g1")
                        else:
                            supervisor.stop_all()

                    start_done = threading.Event()
                    result = []

                    def start_actor() -> None:
                        result.append(self._start(supervisor))
                        start_done.set()

                    with patch.object(module, "PtySession", NewSession):
                        stop_thread = threading.Thread(target=bulk_stop)
                        stop_thread.start()
                        self.assertTrue(stop_entered.wait(timeout=1.0))
                        start_thread = threading.Thread(target=start_actor)
                        start_thread.start()
                        completed_before_stop = start_done.wait(timeout=0.1)
                        release_stop.set()
                        stop_thread.join(timeout=2.0)
                        start_thread.join(timeout=2.0)

                    self.assertFalse(completed_before_stop)
                    self.assertFalse(stop_thread.is_alive())
                    self.assertFalse(start_thread.is_alive())
                    self.assertEqual(old.stop_calls, 1)
                    self.assertEqual(len(created), 1)
                    self.assertEqual(result, created)

    def test_stop_group_does_not_block_start_in_other_group(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class BlockingSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                old = BlockingSession(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[(old.group_id, old.actor_id)] = old

                created = []

                class NewSession(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        created.append(self)

                start_done = threading.Event()

                def start_other_group() -> None:
                    self._start_for(supervisor, group_id="g2", actor_id="a2")
                    start_done.set()

                with patch.object(module, "PtySession", NewSession):
                    stop_thread = threading.Thread(target=lambda: supervisor.stop_group(group_id="g1"))
                    stop_thread.start()
                    self.assertTrue(stop_entered.wait(timeout=1.0))
                    start_thread = threading.Thread(target=start_other_group)
                    start_thread.start()
                    completed_while_g1_stopped = start_done.wait(timeout=0.2)
                    release_stop.set()
                    stop_thread.join(timeout=2.0)
                    start_thread.join(timeout=2.0)

                self.assertTrue(completed_while_g1_stopped)
                self.assertFalse(stop_thread.is_alive())
                self.assertFalse(start_thread.is_alive())
                self.assertEqual([(session.group_id, session.actor_id) for session in created], [("g2", "a2")])

    def test_stop_group_does_not_block_stop_actor_in_other_group(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class BlockingSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                g1_session = BlockingSession(group_id="g1", actor_id="a1")
                g2_session = _SessionState(group_id="g2", actor_id="a2")
                with supervisor._lock:
                    supervisor._sessions[(g1_session.group_id, g1_session.actor_id)] = g1_session
                    supervisor._sessions[(g2_session.group_id, g2_session.actor_id)] = g2_session

                other_stop_done = threading.Event()
                stop_group_thread = threading.Thread(target=lambda: supervisor.stop_group(group_id="g1"))
                stop_group_thread.start()
                self.assertTrue(stop_entered.wait(timeout=1.0))
                stop_actor_thread = threading.Thread(
                    target=lambda: (
                        supervisor.stop_actor(group_id="g2", actor_id="a2"),
                        other_stop_done.set(),
                    )
                )
                stop_actor_thread.start()
                completed_while_g1_stopped = other_stop_done.wait(timeout=0.2)
                release_stop.set()
                stop_group_thread.join(timeout=2.0)
                stop_actor_thread.join(timeout=2.0)

                self.assertTrue(completed_while_g1_stopped)
                self.assertFalse(stop_group_thread.is_alive())
                self.assertFalse(stop_actor_thread.is_alive())
                self.assertEqual(g2_session.stop_calls, 1)

    def test_stop_group_does_not_wait_for_other_group_construction(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                construction_entered = threading.Event()
                release_construction = threading.Event()

                g1_session = _SessionState(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[(g1_session.group_id, g1_session.actor_id)] = g1_session

                class BlockingConstructor(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])
                        construction_entered.set()
                        release_construction.wait(timeout=2.0)

                stop_done = threading.Event()
                with patch.object(module, "PtySession", BlockingConstructor):
                    start_thread = threading.Thread(
                        target=lambda: self._start_for(supervisor, group_id="g2", actor_id="a2")
                    )
                    start_thread.start()
                    self.assertTrue(construction_entered.wait(timeout=1.0))
                    stop_thread = threading.Thread(
                        target=lambda: (supervisor.stop_group(group_id="g1"), stop_done.set())
                    )
                    stop_thread.start()
                    completed_during_g2_construction = stop_done.wait(timeout=0.2)
                    release_construction.set()
                    start_thread.join(timeout=2.0)
                    stop_thread.join(timeout=2.0)

                self.assertTrue(completed_during_g2_construction)
                self.assertFalse(start_thread.is_alive())
                self.assertFalse(stop_thread.is_alive())
                self.assertEqual(g1_session.stop_calls, 1)

    def test_stop_groups_can_run_concurrently(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class BlockingSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                g1_session = BlockingSession(group_id="g1", actor_id="a1")
                g2_session = _SessionState(group_id="g2", actor_id="a2")
                with supervisor._lock:
                    supervisor._sessions[(g1_session.group_id, g1_session.actor_id)] = g1_session
                    supervisor._sessions[(g2_session.group_id, g2_session.actor_id)] = g2_session

                g2_done = threading.Event()
                g1_thread = threading.Thread(target=lambda: supervisor.stop_group(group_id="g1"))
                g1_thread.start()
                self.assertTrue(stop_entered.wait(timeout=1.0))
                g2_thread = threading.Thread(
                    target=lambda: (supervisor.stop_group(group_id="g2"), g2_done.set())
                )
                g2_thread.start()
                g2_completed_while_g1_stopped = g2_done.wait(timeout=0.2)
                release_stop.set()
                g1_thread.join(timeout=2.0)
                g2_thread.join(timeout=2.0)

                self.assertTrue(g2_completed_while_g1_stopped)
                self.assertFalse(g1_thread.is_alive())
                self.assertFalse(g2_thread.is_alive())
                self.assertEqual(g2_session.stop_calls, 1)

    def test_stop_all_still_blocks_start_in_other_group(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class BlockingSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                old = BlockingSession(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[(old.group_id, old.actor_id)] = old

                class NewSession(_SessionState):
                    def __init__(self, **kwargs) -> None:
                        super().__init__(group_id=kwargs["group_id"], actor_id=kwargs["actor_id"])

                start_done = threading.Event()
                with patch.object(module, "PtySession", NewSession):
                    stop_thread = threading.Thread(target=supervisor.stop_all)
                    stop_thread.start()
                    self.assertTrue(stop_entered.wait(timeout=1.0))
                    start_thread = threading.Thread(
                        target=lambda: (
                            self._start_for(supervisor, group_id="g2", actor_id="a2"),
                            start_done.set(),
                        )
                    )
                    start_thread.start()
                    completed_before_stop_all = start_done.wait(timeout=0.2)
                    release_stop.set()
                    stop_thread.join(timeout=2.0)
                    start_thread.join(timeout=2.0)

                self.assertFalse(completed_before_stop_all)
                self.assertTrue(start_done.is_set())
                self.assertFalse(stop_thread.is_alive())
                self.assertFalse(start_thread.is_alive())

    def test_stop_all_still_blocks_stop_group_for_other_group(self) -> None:
        for backend, module in self._backends():
            with self.subTest(backend=backend):
                supervisor = module.PtySupervisor()
                stop_entered = threading.Event()
                release_stop = threading.Event()

                class BlockingSession(_SessionState):
                    def stop(self) -> None:
                        self.stop_calls += 1
                        stop_entered.set()
                        release_stop.wait(timeout=2.0)
                        self.running = False

                old = BlockingSession(group_id="g1", actor_id="a1")
                with supervisor._lock:
                    supervisor._sessions[(old.group_id, old.actor_id)] = old

                other_group_done = threading.Event()
                stop_all_thread = threading.Thread(target=supervisor.stop_all)
                stop_all_thread.start()
                self.assertTrue(stop_entered.wait(timeout=1.0))
                stop_group_thread = threading.Thread(
                    target=lambda: (
                        supervisor.stop_group(group_id="g2"),
                        other_group_done.set(),
                    )
                )
                stop_group_thread.start()
                completed_before_stop_all = other_group_done.wait(timeout=0.2)
                release_stop.set()
                stop_all_thread.join(timeout=2.0)
                stop_group_thread.join(timeout=2.0)

                self.assertFalse(completed_before_stop_all)
                self.assertTrue(other_group_done.is_set())
                self.assertFalse(stop_all_thread.is_alive())
                self.assertFalse(stop_group_thread.is_alive())


if __name__ == "__main__":
    unittest.main()
