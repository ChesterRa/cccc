"""Group-scoped Presentation state operations for daemon."""

from __future__ import annotations

import mimetypes
from pathlib import Path, PurePosixPath
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import urlparse

from ...contracts.v1 import (
    DaemonError,
    DaemonResponse,
    PresentationCard,
    PresentationCardType,
    PresentationContent,
    PresentationSlot,
    PresentationSnapshot,
    PresentationTableData,
)
from ...kernel.actors import find_actor
from ...kernel.blobs import resolve_blob_attachment_path, sanitize_filename, store_blob_bytes
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.prompt_files import resolve_active_scope_root
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso
from .presentation_browser_runtime import close_browser_surface_session

_PRESENTATION_VERSION = 1
_SLOT_IDS = tuple(f"slot-{index}" for index in range(1, 5))
_SUPPORTED_CARD_TYPES = {"markdown", "table", "image", "pdf", "file", "web_preview"}
_IMAGE_SUFFIXES = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif"}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _presentation_state_path(group_id: str) -> Path:
    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    return group.path / "state" / "presentation.json"


def _empty_snapshot(*, now: str = "") -> PresentationSnapshot:
    ts = str(now or "")
    return PresentationSnapshot(
        v=_PRESENTATION_VERSION,
        updated_at=ts,
        highlight_slot_id="",
        slots=[PresentationSlot(slot_id=slot_id, index=index) for index, slot_id in enumerate(_SLOT_IDS, start=1)],
    )


def load_presentation_snapshot(group_id: str) -> PresentationSnapshot:
    path = _presentation_state_path(group_id)
    raw = read_json(path)
    if not raw:
        return _empty_snapshot()
    try:
        parsed = PresentationSnapshot.model_validate(raw)
    except Exception:
        return _empty_snapshot()

    slots_by_id = {
        str(slot.slot_id or "").strip(): slot
        for slot in parsed.slots
        if str(slot.slot_id or "").strip() in _SLOT_IDS
    }
    ordered_slots: List[PresentationSlot] = []
    for index, slot_id in enumerate(_SLOT_IDS, start=1):
        existing = slots_by_id.get(slot_id)
        if existing is None:
            ordered_slots.append(PresentationSlot(slot_id=slot_id, index=index))
            continue
        ordered_slots.append(
            PresentationSlot(
                slot_id=slot_id,
                index=index,
                card=existing.card,
            )
        )

    highlight_slot_id = str(parsed.highlight_slot_id or "").strip()
    if highlight_slot_id not in _SLOT_IDS:
        highlight_slot_id = ""
    return PresentationSnapshot(
        v=_PRESENTATION_VERSION,
        updated_at=str(parsed.updated_at or "").strip(),
        highlight_slot_id=highlight_slot_id,
        slots=ordered_slots,
    )


def _write_snapshot(group_id: str, snapshot: PresentationSnapshot) -> None:
    path = _presentation_state_path(group_id)
    atomic_write_json(path, snapshot.model_dump(mode="json", exclude_none=True), indent=2)


def _close_browser_surface_session_quietly(group_id: str, slot_id: str) -> None:
    try:
        close_browser_surface_session(group_id=group_id, slot_id=slot_id)
    except Exception:
        pass


def _validate_publisher(group: Any, by: str) -> None:
    who = str(by or "").strip()
    if not who or who in {"user", "system"}:
        return
    if not isinstance(find_actor(group, who), dict):
        raise ValueError(f"unknown actor: {who}")


def _normalize_slot_id(value: Any, *, allow_auto: bool) -> str:
    raw = str(value or "").strip().lower()
    if not raw:
        return "auto" if allow_auto else ""
    if allow_auto and raw == "auto":
        return "auto"
    if raw.isdigit():
        normalized = f"slot-{int(raw)}"
        return normalized if normalized in _SLOT_IDS else ""
    if raw in _SLOT_IDS:
        return raw
    if raw.startswith("slot") and raw.replace("_", "-") in _SLOT_IDS:
        return raw.replace("_", "-")
    return ""


def _select_auto_slot(snapshot: PresentationSnapshot) -> str:
    for slot in snapshot.slots:
        if slot.card is None:
            return slot.slot_id
    oldest = min(
        snapshot.slots,
        key=lambda slot: str(slot.card.published_at if slot.card else "") or "~",
    )
    return oldest.slot_id


def _resolve_target_slot(snapshot: PresentationSnapshot, requested: Any) -> str:
    slot_id = _normalize_slot_id(requested, allow_auto=True)
    if slot_id == "auto":
        return _select_auto_slot(snapshot)
    if slot_id in _SLOT_IDS:
        return slot_id
    raise ValueError("slot must be auto or one of: slot-1, slot-2, slot-3, slot-4")


def _resolve_scope_file(group: Any, raw_path: str) -> Tuple[Path, str]:
    root = resolve_active_scope_root(group)
    if root is None:
        raise ValueError("group has no active scope")

    src = Path(str(raw_path or "").strip()).expanduser()
    if not src.is_absolute():
        src = (root / src).resolve()
    else:
        src = src.resolve()
    try:
        rel = src.relative_to(root)
    except ValueError as exc:
        raise ValueError("path must be under the group's active scope root") from exc
    if not src.exists() or not src.is_file():
        raise ValueError(f"file not found: {src}")
    return src, rel.as_posix()


def resolve_workspace_asset_path(group: Any, workspace_rel_path: str) -> Path:
    root = resolve_active_scope_root(group)
    if root is None:
        raise ValueError("group has no active scope")

    rel_text = str(workspace_rel_path or "").strip().replace("\\", "/")
    if not rel_text:
        raise ValueError("missing workspace_rel_path")
    rel_path = PurePosixPath(rel_text)
    if rel_path.is_absolute() or ".." in rel_path.parts:
        raise ValueError("workspace_rel_path must stay under the group's active scope root")

    candidate = (root / Path(*rel_path.parts)).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise ValueError("workspace_rel_path must stay under the group's active scope root") from exc
    if not candidate.exists() or not candidate.is_file():
        raise FileNotFoundError(f"file not found: {candidate}")
    return candidate


def _path_suffix_hint(path_text: str) -> str:
    try:
        parsed = urlparse(path_text)
        candidate = parsed.path or path_text
    except Exception:
        candidate = path_text
    return Path(str(candidate or "")).suffix.lower()


def _guess_card_type(*, explicit: str, path: str, url: str, content: str, table: Any) -> str:
    wanted = str(explicit or "").strip().lower()
    if wanted:
        if wanted not in _SUPPORTED_CARD_TYPES:
            raise ValueError(f"unsupported card_type: {wanted}")
        return wanted

    if isinstance(table, dict) or isinstance(table, list):
        return "table"

    path_hint = path or url
    suffix = _path_suffix_hint(path_hint)
    if suffix in {".md", ".markdown"}:
        return "markdown"
    if suffix in _IMAGE_SUFFIXES:
        return "image"
    if suffix == ".pdf":
        return "pdf"
    if suffix in {".html", ".htm"}:
        return "web_preview"
    if str(content or "").strip():
        return "markdown"
    if str(path or "").strip() or str(url or "").strip():
        return "file"
    raise ValueError("unable to infer card_type; pass card_type explicitly")


def _derive_title(*, title: str, path: str, url: str, card_type: str) -> str:
    if str(title or "").strip():
        return str(title).strip()
    if str(path or "").strip():
        return Path(str(path).strip()).name or f"{card_type}"
    if str(url or "").strip():
        parsed = urlparse(str(url).strip())
        if parsed.path:
            name = Path(parsed.path).name
            if name:
                return name
        if parsed.netloc:
            return parsed.netloc
    return card_type.replace("_", " ")


def _normalize_table(raw: Any) -> PresentationTableData:
    if isinstance(raw, dict):
        columns_raw = raw.get("columns")
        rows_raw = raw.get("rows")
        if isinstance(columns_raw, list) and isinstance(rows_raw, list):
            columns = [str(item or "").strip() for item in columns_raw]
            rows = []
            for row in rows_raw:
                if isinstance(row, list):
                    rows.append([str(item or "") for item in row])
                elif isinstance(row, dict):
                    rows.append([str(row.get(column) or "") for column in columns])
            return PresentationTableData(columns=columns, rows=rows)
        if isinstance(rows_raw, list) and rows_raw and all(isinstance(item, dict) for item in rows_raw):
            keys: List[str] = []
            for row in rows_raw:
                assert isinstance(row, dict)
                for key in row.keys():
                    key_text = str(key or "").strip()
                    if key_text and key_text not in keys:
                        keys.append(key_text)
            rows = [[str((row if isinstance(row, dict) else {}).get(key) or "") for key in keys] for row in rows_raw]
            return PresentationTableData(columns=keys, rows=rows)
    if isinstance(raw, list) and raw and all(isinstance(item, dict) for item in raw):
        keys: List[str] = []
        for row in raw:
            assert isinstance(row, dict)
            for key in row.keys():
                key_text = str(key or "").strip()
                if key_text and key_text not in keys:
                    keys.append(key_text)
        rows = [[str((row if isinstance(row, dict) else {}).get(key) or "") for key in keys] for row in raw]
        return PresentationTableData(columns=keys, rows=rows)
    raise ValueError("table must be {columns, rows} or a list of row objects")


def _normalize_blob_reference(group: Any, blob_rel_path: str) -> Tuple[str, str]:
    rel = str(blob_rel_path or "").strip()
    if not rel:
        raise ValueError("missing blob_rel_path")
    if not rel.startswith("state/blobs/"):
        rel = f"state/blobs/{Path(rel).name}"
    path = resolve_blob_attachment_path(group, rel_path=rel)
    mime_type, _ = mimetypes.guess_type(path.name)
    return rel, str(mime_type or "")


def _build_card(
    *,
    group: Any,
    slot_id: str,
    card_type: str,
    title: str,
    by: str,
    summary: str,
    source_label: str,
    source_ref: str,
    content: str,
    table: Any,
    path: str,
    url: str,
    blob_rel_path: str,
) -> PresentationCard:
    now = utc_now_iso()
    title_text = _derive_title(title=title, path=path, url=url, card_type=card_type)
    summary_text = str(summary or "").strip()
    source_label_text = str(source_label or "").strip()
    source_ref_text = str(source_ref or "").strip()
    content_text = str(content or "")
    path_text = str(path or "").strip()
    url_text = str(url or "").strip()
    blob_rel_path_text = str(blob_rel_path or "").strip()

    if card_type == "markdown":
        if path_text:
            src, workspace_rel_path = _resolve_scope_file(group, path_text)
            mime_type = str(mimetypes.guess_type(src.name)[0] or "text/markdown")
            source_label_text = source_label_text or src.name
            source_ref_text = source_ref_text or workspace_rel_path
            return PresentationCard(
                slot_id=slot_id,
                title=title_text,
                card_type="markdown",
                published_by=by,
                published_at=now,
                source_label=source_label_text,
                source_ref=source_ref_text,
                summary=summary_text,
                content=PresentationContent(
                    mode="workspace_link",
                    workspace_rel_path=workspace_rel_path,
                    mime_type=mime_type or None,
                    file_name=src.name,
                ),
            )
        if not content_text:
            raise ValueError("markdown card requires content or path")
        return PresentationCard(
            slot_id=slot_id,
            title=title_text,
            card_type="markdown",
            published_by=by,
            published_at=now,
            source_label=source_label_text,
            source_ref=source_ref_text,
            summary=summary_text,
            content=PresentationContent(mode="inline", markdown=content_text),
        )

    if card_type == "table":
        table_doc = _normalize_table(table)
        return PresentationCard(
            slot_id=slot_id,
            title=title_text,
            card_type="table",
            published_by=by,
            published_at=now,
            source_label=source_label_text,
            source_ref=source_ref_text,
            summary=summary_text,
            content=PresentationContent(mode="inline", table=table_doc),
        )

    if card_type == "web_preview":
        mime_type = ""
        if path_text:
            src, workspace_rel_path = _resolve_scope_file(group, path_text)
            mime_type = str(mimetypes.guess_type(src.name)[0] or "text/html")
            source_label_text = source_label_text or src.name
            source_ref_text = source_ref_text or workspace_rel_path
            return PresentationCard(
                slot_id=slot_id,
                title=title_text,
                card_type="web_preview",
                published_by=by,
                published_at=now,
                source_label=source_label_text,
                source_ref=source_ref_text,
                summary=summary_text,
                content=PresentationContent(
                    mode="workspace_link",
                    workspace_rel_path=workspace_rel_path,
                    mime_type=mime_type or None,
                    file_name=src.name,
                ),
            )
        elif content_text and not url_text and not blob_rel_path_text:
            safe_name = sanitize_filename(f"{title_text}.html", fallback="presentation.html")
            stored = store_blob_bytes(group, data=content_text.encode("utf-8"), filename=safe_name, mime_type="text/html")
            blob_rel_path_text = str(stored.get("path") or "")
            mime_type = "text/html"
            source_label_text = source_label_text or safe_name
            source_ref_text = source_ref_text or "inline-html"
        elif blob_rel_path_text:
            blob_rel_path_text, mime_type = _normalize_blob_reference(group, blob_rel_path_text)
        elif url_text:
            source_ref_text = source_ref_text or url_text
        else:
            raise ValueError("web_preview card requires path, url, blob_rel_path, or inline content")

        return PresentationCard(
            slot_id=slot_id,
            title=title_text,
            card_type="web_preview",
            published_by=by,
            published_at=now,
            source_label=source_label_text,
            source_ref=source_ref_text,
            summary=summary_text,
            content=PresentationContent(
                mode="reference",
                url=url_text or None,
                blob_rel_path=blob_rel_path_text or None,
                mime_type=mime_type or None,
                file_name=source_label_text or None,
            ),
        )

    mime_type = ""
    file_name = ""
    if path_text:
        src, workspace_rel_path = _resolve_scope_file(group, path_text)
        mime_type = str(mimetypes.guess_type(src.name)[0] or "")
        file_name = src.name
        source_label_text = source_label_text or src.name
        source_ref_text = source_ref_text or workspace_rel_path
        return PresentationCard(
            slot_id=slot_id,
            title=title_text,
            card_type="image" if card_type == "image" else "pdf" if card_type == "pdf" else "file",
            published_by=by,
            published_at=now,
            source_label=source_label_text,
            source_ref=source_ref_text,
            summary=summary_text,
            content=PresentationContent(
                mode="workspace_link",
                workspace_rel_path=workspace_rel_path,
                mime_type=mime_type or None,
                file_name=file_name or title_text,
            ),
        )
    elif blob_rel_path_text:
        blob_rel_path_text, mime_type = _normalize_blob_reference(group, blob_rel_path_text)
        file_name = Path(blob_rel_path_text).name
        source_ref_text = source_ref_text or blob_rel_path_text
        source_label_text = source_label_text or file_name
    elif url_text:
        suffix = _path_suffix_hint(url_text)
        mime_type = str(mimetypes.guess_type(f"file{suffix}")[0] or "")
        file_name = Path(urlparse(url_text).path or "").name or title_text
        source_ref_text = source_ref_text or url_text
        source_label_text = source_label_text or file_name
    else:
        raise ValueError(f"{card_type} card requires path, url, or blob_rel_path")

    normalized_card_type: PresentationCardType = "file"
    if card_type == "image":
        normalized_card_type = "image"
    elif card_type == "pdf":
        normalized_card_type = "pdf"

    return PresentationCard(
        slot_id=slot_id,
        title=title_text,
        card_type=normalized_card_type,
        published_by=by,
        published_at=now,
        source_label=source_label_text,
        source_ref=source_ref_text,
        summary=summary_text,
        content=PresentationContent(
            mode="reference",
            url=url_text or None,
            blob_rel_path=blob_rel_path_text or None,
            mime_type=mime_type or None,
            file_name=file_name or title_text,
        ),
    )


def handle_presentation_get(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    snapshot = load_presentation_snapshot(group.group_id)
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "presentation": snapshot.model_dump(mode="json", exclude_none=True)})


def handle_presentation_publish(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _validate_publisher(group, by)
        snapshot = load_presentation_snapshot(group.group_id)
        path_text = str(args.get("path") or "").strip()
        url_text = str(args.get("url") or "").strip()
        content_text = str(args.get("content") or "")
        table_raw = args.get("table")
        blob_rel_path_text = str(args.get("blob_rel_path") or "").strip()
        card_type = _guess_card_type(
            explicit=str(args.get("card_type") or ""),
            path=path_text,
            url=url_text,
            content=content_text,
            table=table_raw,
        )
        slot_id = _resolve_target_slot(snapshot, args.get("slot"))
        card = _build_card(
            group=group,
            slot_id=slot_id,
            card_type=card_type,
            title=str(args.get("title") or ""),
            by=by,
            summary=str(args.get("summary") or ""),
            source_label=str(args.get("source_label") or ""),
            source_ref=str(args.get("source_ref") or ""),
            content=content_text,
            table=table_raw,
            path=path_text,
            url=url_text,
            blob_rel_path=blob_rel_path_text,
        )
        replacing_existing_card = any(slot.slot_id == slot_id and slot.card is not None for slot in snapshot.slots)
    except Exception as exc:
        return _error("presentation_publish_failed", str(exc))

    next_slots: List[PresentationSlot] = []
    for slot in snapshot.slots:
        if slot.slot_id == slot_id:
            next_slots.append(PresentationSlot(slot_id=slot.slot_id, index=slot.index, card=card))
        else:
            next_slots.append(slot)
    next_snapshot = PresentationSnapshot(
        v=_PRESENTATION_VERSION,
        updated_at=utc_now_iso(),
        highlight_slot_id=slot_id,
        slots=next_slots,
    )
    try:
        if replacing_existing_card:
            _close_browser_surface_session_quietly(group.group_id, slot_id)
        _write_snapshot(group.group_id, next_snapshot)
        event = append_event(
            group.ledger_path,
            kind="presentation.publish",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "slot_id": slot_id,
                "title": card.title,
                "card_type": card.card_type,
                "source_label": card.source_label,
                "source_ref": card.source_ref,
                "summary": card.summary,
            },
        )
    except Exception as exc:
        return _error("presentation_publish_failed", str(exc))
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "slot_id": slot_id,
            "card": card.model_dump(mode="json", exclude_none=True),
            "presentation": next_snapshot.model_dump(mode="json", exclude_none=True),
            "event": event,
        },
    )


def handle_presentation_clear(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        _validate_publisher(group, by)
        snapshot = load_presentation_snapshot(group.group_id)
        raw_slot = args.get("slot")
        clear_all = bool(args.get("all") is True)
        if clear_all:
            requested_slots = list(_SLOT_IDS)
        else:
            slot_id = _normalize_slot_id(raw_slot, allow_auto=False)
            requested_slots = [slot_id] if slot_id else list(_SLOT_IDS)
        if any(slot_id not in _SLOT_IDS for slot_id in requested_slots):
            raise ValueError("slot must be one of: slot-1, slot-2, slot-3, slot-4")
    except Exception as exc:
        return _error("presentation_clear_failed", str(exc))

    cleared_slots: List[str] = []
    next_slots: List[PresentationSlot] = []
    for slot in snapshot.slots:
        if slot.slot_id in requested_slots and slot.card is not None:
            cleared_slots.append(slot.slot_id)
            next_slots.append(PresentationSlot(slot_id=slot.slot_id, index=slot.index))
        else:
            next_slots.append(slot)

    highlight_slot_id = snapshot.highlight_slot_id
    if highlight_slot_id in cleared_slots:
        highlight_slot_id = ""

    next_snapshot = PresentationSnapshot(
        v=_PRESENTATION_VERSION,
        updated_at=utc_now_iso(),
        highlight_slot_id=highlight_slot_id,
        slots=next_slots,
    )
    try:
        _write_snapshot(group.group_id, next_snapshot)
        for cleared_slot_id in cleared_slots:
            _close_browser_surface_session_quietly(group.group_id, cleared_slot_id)
        event = append_event(
            group.ledger_path,
            kind="presentation.clear",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={
                "slot_id": cleared_slots[0] if len(cleared_slots) == 1 else "",
                "cleared_all": len(requested_slots) == len(_SLOT_IDS),
                "cleared_slots": cleared_slots,
            },
        )
    except Exception as exc:
        return _error("presentation_clear_failed", str(exc))
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group.group_id,
            "cleared_slots": cleared_slots,
            "presentation": next_snapshot.model_dump(mode="json", exclude_none=True),
            "event": event,
        },
    )


def try_handle_presentation_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "presentation_get":
        return handle_presentation_get(args)
    if op == "presentation_publish":
        return handle_presentation_publish(args)
    if op == "presentation_clear":
        return handle_presentation_clear(args)
    return None
