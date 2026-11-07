from enum import StrEnum


class OutputStream(StrEnum):
    STDERR = "stderr"
    STDOUT = "stdout"

    def __str__(self) -> str:
        return str(self.value)
