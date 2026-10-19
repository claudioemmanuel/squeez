---
description: Mostra o pato companheiro (card, stats, carinho, renomear)
---
O usuário quer interagir com o pato companheiro do squeez. Pedido: $ARGUMENTS

Rode o CLI do pato e mostre a saída ao usuário **verbatim, em um bloco de
código** (a arte ASCII depende do alinhamento):

- Sem argumentos ou "card": `node ~/.claude/squeez/buddy/buddy-cli.js card`
- "stats" ou "status": `node ~/.claude/squeez/buddy/buddy-cli.js stats`
- "pet" / "carinho": `node ~/.claude/squeez/buddy/buddy-cli.js pet`
- "rename <nome>" / "chama ele de <nome>": `node ~/.claude/squeez/buddy/buddy-cli.js rename <nome>`
- "reset" / "zera o XP": `node ~/.claude/squeez/buddy/buddy-cli.js reset`

Depois do bloco, no máximo uma linha de comentário — o pato fala por si.

A fonte principal de XP é a economia de tokens do squeez: 25.000 tokens
líquidos economizados = 1 XP, sem teto. Ações (edits, commits, suíte que sai do
vermelho) valem pouco e param num teto de 8 XP por sessão — subir de rank tem
que vir de economia real, não de grind. O desenho muda a cada cor de raridade,
então dá pra ver o progresso: filhote (Comum) → jovem (Incomum) → adulto na água
(Raro) → asa aberta (Épico) → coroado (Lendário).
