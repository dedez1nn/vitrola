"""Regras de leitura do histórico, compartilhadas por stats e recommend (aplicadas na leitura, não na gravação)."""

MIN_SECONDS = 30
MIN_FRACTION = 0.5


def counted(listened: float | None, duration: float | None) -> bool:
    """Uma reprodução 'conta' com 30 s ou 50% da faixa, o que vier primeiro."""
    if listened is None:
        return False
    need = MIN_SECONDS if not duration else min(MIN_SECONDS, MIN_FRACTION * duration)
    return listened >= need
