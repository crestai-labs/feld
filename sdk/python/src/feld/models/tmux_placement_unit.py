from enum import StrEnum


class TmuxPlacementUnit(StrEnum):
    PANE = "pane"
    SESSION = "session"
    WINDOW = "window"

    def __str__(self) -> str:
        return str(self.value)
