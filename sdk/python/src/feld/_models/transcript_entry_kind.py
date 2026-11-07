from enum import StrEnum


class TranscriptEntryKind(StrEnum):
    INPUT = "input"
    NOTIFICATION = "notification"
    OTHER = "other"
    RESPONSE = "response"
    SYSTEM = "system"
    THINKING = "thinking"
    TOOL_CALL = "tool-call"
    TOOL_RESULT = "tool-result"

    def __str__(self) -> str:
        return str(self.value)
