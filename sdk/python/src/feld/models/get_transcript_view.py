from enum import StrEnum


class GetTranscriptView(StrEnum):
    ALL = "all"
    CONVERSATION = "conversation"

    def __str__(self) -> str:
        return str(self.value)
