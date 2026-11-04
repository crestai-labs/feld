from enum import StrEnum


class HerdrPlacementPaneOrigin(StrEnum):
    CREATED = "created"
    EXISTING = "existing"

    def __str__(self) -> str:
        return str(self.value)
