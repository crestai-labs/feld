from enum import StrEnum


class DeclareResponseAction(StrEnum):
    CREATE = "create"
    REPLACE = "replace"
    UNCHANGED = "unchanged"

    def __str__(self) -> str:
        return str(self.value)
