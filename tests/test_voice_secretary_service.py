import os
from pathlib import Path

import pytest


@pytest.mark.skipif(os.name != "nt", reason="Windows path parsing regression test")
def test_command_argv_preserves_windows_audio_path() -> None:
    from cccc.daemon.assistants.voice_secretary_service import _command_argv

    audio_path = Path(r"C:\Users\example\AppData\Local\Temp\cccc-voice-secretary-test.webm")

    argv = _command_argv(
        r'"C:\Program Files\Python\python.exe" "C:\Model Dir\adapter.py" {audio_path}',
        audio_path=audio_path,
        mime_type="audio/webm",
        language="en-US",
    )

    assert argv == [
        r"C:\Program Files\Python\python.exe",
        r"C:\Model Dir\adapter.py",
        str(audio_path),
    ]


@pytest.mark.skipif(os.name != "nt", reason="Windows command template regression test")
def test_list_command_template_renders_parseable_windows_paths() -> None:
    from cccc.daemon.assistants.voice_models import _render_command_template
    from cccc.daemon.assistants.voice_secretary_service import _command_argv

    model_dir = Path(r"C:\Users\example\AppData\Local\Temp\cache\voice-models\mock_asr")
    audio_path = Path(r"C:\Users\example\AppData\Local\Temp\cccc-voice-secretary-test.webm")

    command = _render_command_template(
        ["{python}", "{model_dir}/adapter.py", "{audio_path}"],
        model_id="mock_asr",
        model_dir=model_dir,
    )
    argv = _command_argv(command, audio_path=audio_path, mime_type="audio/webm", language="en-US")

    assert Path(argv[1]) == model_dir / "adapter.py"
    assert Path(argv[2]) == audio_path
