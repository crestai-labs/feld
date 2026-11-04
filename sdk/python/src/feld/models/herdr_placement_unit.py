from enum import StrEnum


class HerdrPlacementUnit(StrEnum):
    PANE = "pane"
    TAB = "tab"
    WORKSPACE = "workspace"

    def __str__(self) -> str:
        return str(self.value)
