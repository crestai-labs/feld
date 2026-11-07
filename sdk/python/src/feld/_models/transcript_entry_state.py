from enum import StrEnum


class TranscriptEntryState(StrEnum):
    LOST = "lost"
    READABLE = "readable"

    def __str__(self) -> str:
        return str(self.value)
