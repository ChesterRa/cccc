"""Message schema used by ReMe file-based context checker/compactor."""

from pydantic import BaseModel, Field

from ..enumeration import Role


class Message(BaseModel):
    role: Role = Field(default=Role.ASSISTANT)
    content: str = Field(default="")
    name: str | None = Field(default=None)

