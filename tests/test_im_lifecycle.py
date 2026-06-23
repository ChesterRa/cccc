from __future__ import annotations

from typing import Any, Dict, List, Optional

from cccc.ports.im.adapters.base import IMAdapter, IMProcessingContext, IMProcessingOutcome


class DummyAdapter(IMAdapter):
    platform = "dummy"

    def connect(self) -> bool:
        return True

    def disconnect(self) -> None:
        return None

    def poll(self) -> List[Dict[str, Any]]:
        return []

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return chat_id


def test_processing_lifecycle_defaults_are_noops() -> None:
    adapter = DummyAdapter()
    ctx = IMProcessingContext(
        chat_id="chat-1",
        thread_id=0,
        message_id="msg-1",
        platform="dummy",
    )

    handle = adapter.on_processing_start(ctx)
    adapter.on_processing_complete(ctx, IMProcessingOutcome.SUCCESS, handle)

    assert handle is None


class LifecycleAdapter(DummyAdapter):
    def __init__(self) -> None:
        self.reactions: List[tuple[str, str]] = []
        self.removed: List[tuple[str, str]] = []
        self.actions: List[tuple[str, str]] = []

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
        self.reactions.append((message_id, emoji_type))
        return f"{message_id}:{emoji_type}"

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        self.removed.append((message_id, reaction_id))
        return True

    def send_chat_action(self, chat_id: str, action: str = "typing") -> bool:
        self.actions.append((chat_id, action))
        return True


def test_processing_lifecycle_starts_and_completes_reaction() -> None:
    from cccc.ports.im.lifecycle import IMProcessingLifecycle

    adapter = LifecycleAdapter()
    lifecycle = IMProcessingLifecycle(adapter)

    lifecycle.start(chat_id="chat-1", thread_id=0, message_id="msg-1")
    lifecycle.complete("chat-1", IMProcessingOutcome.SUCCESS)

    assert adapter.reactions == [("msg-1", "")]
    assert adapter.removed == [("msg-1", "msg-1:")]


def test_processing_lifecycle_refreshes_typing_actions() -> None:
    from cccc.ports.im.lifecycle import IMProcessingLifecycle

    adapter = LifecycleAdapter()
    lifecycle = IMProcessingLifecycle(adapter)

    lifecycle.start(chat_id="chat-1", thread_id=0, message_id="")
    lifecycle.refresh()

    assert ("chat-1", "typing") in adapter.actions


def test_prepare_inbound_content_uses_attachment_title_when_text_empty(tmp_path) -> None:
    from cccc.ports.im.inbound_content import prepare_inbound_content

    class Adapter:
        platform = "telegram"

        def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
            _ = attachment
            return b"hello"

    class Group:
        path = tmp_path
        doc = {"im": {"files": {"enabled": True, "max_mb": 1}}}

    warnings: List[str] = []
    result = prepare_inbound_content(
        group=Group(),
        adapter=Adapter(),
        text="",
        attachments=[{"file_name": "hello.txt", "mime_type": "text/plain", "kind": "file", "bytes": 5}],
        send_warning=warnings.append,
    )

    assert warnings == []
    assert result.text == "[file] hello.txt"
    assert len(result.attachments) == 1
    assert result.attachments[0]["title"] == "hello.txt"


def test_prepare_inbound_content_honors_string_false_files_enabled(tmp_path) -> None:
    from cccc.ports.im.inbound_content import prepare_inbound_content

    class Adapter:
        platform = "telegram"

        def __init__(self) -> None:
            self.downloads = 0

        def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
            _ = attachment
            self.downloads += 1
            return b"hello"

    class Group:
        path = tmp_path
        doc = {"im": {"files": {"enabled": "false", "max_mb": 1}}}

    adapter = Adapter()
    warnings: List[str] = []

    result = prepare_inbound_content(
        group=Group(),
        adapter=adapter,
        text="incoming",
        attachments=[{"file_name": "hello.txt", "mime_type": "text/plain", "kind": "file", "bytes": 5}],
        send_warning=warnings.append,
    )

    assert warnings == []
    assert result.text == "incoming"
    assert result.attachments == []
    assert adapter.downloads == 0


def test_dingtalk_processing_lifecycle_uses_ai_card_when_available(tmp_path) -> None:
    from cccc.ports.im.adapters.dingtalk import DingTalkAdapter

    class FakeCardClient:
        def __init__(self) -> None:
            self.created: List[tuple[str, str]] = []
            self.finalized: List[tuple[str, str]] = []

        def create_card(self, chat_id: str, text: str) -> str:
            self.created.append((chat_id, text))
            return "card-1"

        def finalize_card(self, card_id: str, text: str) -> bool:
            self.finalized.append((card_id, text))
            return True

    adapter = DingTalkAdapter(
        app_key="app",
        app_secret="secret",
        robot_code="robot",
        session_state_path=tmp_path / "sessions.json",
    )
    fake_client = FakeCardClient()
    adapter._get_card_client = lambda: fake_client  # type: ignore[method-assign]
    adapter._run_async = lambda result: result  # type: ignore[method-assign]
    ctx = IMProcessingContext(chat_id="cid-1", message_id="msg-1", platform="dingtalk")

    handle = adapter.on_processing_start(ctx)
    adapter.on_processing_complete(ctx, IMProcessingOutcome.SUCCESS, handle)

    assert handle == "dingtalk_card:card-1"
    assert fake_client.created == [("cid-1", "处理中...")]
    assert fake_client.finalized == [("card-1", "处理完成")]


def test_dingtalk_processing_lifecycle_logs_card_create_result(tmp_path) -> None:
    from cccc.ports.im.adapters.dingtalk import DingTalkAdapter

    class FakeCardClient:
        def create_card(self, chat_id: str, text: str) -> str:
            _ = chat_id, text
            return "card-1"

    adapter = DingTalkAdapter(
        app_key="app",
        app_secret="secret",
        robot_code="robot",
        log_path=tmp_path / "im_bridge.log",
        session_state_path=tmp_path / "sessions.json",
    )
    adapter._get_card_client = lambda: FakeCardClient()  # type: ignore[method-assign]
    adapter._run_async = lambda result: result  # type: ignore[method-assign]
    ctx = IMProcessingContext(chat_id="cid-1", message_id="msg-1", platform="dingtalk")

    handle = adapter.on_processing_start(ctx)

    assert handle == "dingtalk_card:card-1"
    assert "[processing] dingtalk AI Card created card=card-1 chat=cid-1" in (tmp_path / "im_bridge.log").read_text()


def test_dingtalk_processing_lifecycle_logs_empty_card_id(tmp_path) -> None:
    from cccc.ports.im.adapters.dingtalk import DingTalkAdapter

    class FakeCardClient:
        def create_card(self, chat_id: str, text: str) -> str:
            _ = chat_id, text
            return ""

    adapter = DingTalkAdapter(
        app_key="app",
        app_secret="secret",
        robot_code="robot",
        log_path=tmp_path / "im_bridge.log",
        session_state_path=tmp_path / "sessions.json",
    )
    adapter._get_card_client = lambda: FakeCardClient()  # type: ignore[method-assign]
    adapter._run_async = lambda result: result  # type: ignore[method-assign]
    ctx = IMProcessingContext(chat_id="cid-1", message_id="msg-1", platform="dingtalk")

    handle = adapter.on_processing_start(ctx)

    assert handle is None
    assert "[processing] dingtalk AI Card create returned no card id chat=cid-1" in (tmp_path / "im_bridge.log").read_text()


def test_dingtalk_processing_lifecycle_noops_without_robot_code(tmp_path) -> None:
    from cccc.ports.im.adapters.dingtalk import DingTalkAdapter

    adapter = DingTalkAdapter(
        app_key="app",
        app_secret="secret",
        robot_code="",
        session_state_path=tmp_path / "sessions.json",
    )
    ctx = IMProcessingContext(chat_id="cid-1", message_id="msg-1", platform="dingtalk")

    handle = adapter.on_processing_start(ctx)
    adapter.on_processing_complete(ctx, IMProcessingOutcome.SUCCESS, handle)

    assert handle is None


def test_dingtalk_processing_lifecycle_marks_failure(tmp_path) -> None:
    from cccc.ports.im.adapters.dingtalk import DingTalkAdapter

    class FakeCardClient:
        def __init__(self) -> None:
            self.finalized: List[tuple[str, str]] = []

        def finalize_card(self, card_id: str, text: str) -> bool:
            self.finalized.append((card_id, text))
            return True

    adapter = DingTalkAdapter(
        app_key="app",
        app_secret="secret",
        robot_code="robot",
        session_state_path=tmp_path / "sessions.json",
    )
    fake_client = FakeCardClient()
    adapter._get_card_client = lambda: fake_client  # type: ignore[method-assign]
    adapter._run_async = lambda result: result  # type: ignore[method-assign]
    ctx = IMProcessingContext(chat_id="cid-1", message_id="msg-1", platform="dingtalk")

    adapter.on_processing_complete(ctx, IMProcessingOutcome.FAILURE, "dingtalk_card:card-1")

    assert fake_client.finalized == [("card-1", "处理失败")]
