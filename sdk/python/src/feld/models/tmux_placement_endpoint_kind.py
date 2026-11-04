from enum import StrEnum


class TmuxPlacementEndpointKind(StrEnum):
    NAME = "name"
    PATH = "path"

    def __str__(self) -> str:
        return str(self.value)
