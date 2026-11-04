from enum import StrEnum


class HerdrPlacementSplit(StrEnum):
    DOWN = "down"
    RIGHT = "right"

    def __str__(self) -> str:
        return str(self.value)
