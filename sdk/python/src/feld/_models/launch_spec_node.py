from enum import StrEnum


class LaunchSpecNode(StrEnum):
    LOCAL = "local"

    def __str__(self) -> str:
        return str(self.value)
