# AuraEssentials para Horizon

> **Legado:** este projeto usa mixins/trait mods ligados ao binário e não faz parte do host atual de plugins em runtime. O workflow de release não o injeta mais no engine. O plugin separado em `server-plugins/aura-loja` é o começo da substituição; não copie os mixins deste diretório para o build do servidor.

Mod nativo de servidor em Rust, feito para o Horizon 26.3. O projeto começa com o sistema de locais pessoais e foi organizado para crescer para teleportes, terrenos, lobby protegido, loja e administração.

## Comandos de locais

- `/sethome <nome>` salva a posição atual com esse nome. Repetir o comando com o mesmo nome move o local.
- `/home <nome>` inicia o teleporte para o local salvo.
- `/home` e `/homes` mostram a lista de locais do jogador.
- Ao completar `/home `, o servidor sugere os nomes salvos pelo próprio jogador.
- `/delhome <nome>` remove um local.

Os nomes usam até 16 letras, números, `_` ou `-`. Os locais ficam associados ao UUID e incluem dimensão, coordenadas e orientação. O limite inicial vem da configuração do mod e pode crescer com o tempo jogado.

## Pacote de recursos Java

O mod lê `resource-pack.json` em `world/horizon-auraessentials/` (ou na pasta do mundo definida em `server.toml`). O loader cria o arquivo na primeira inicialização com os dois pacotes Java oficiais do AuraEssentials: Kingdoms como pacote principal e AuraNPC como pacote adicional. Exemplo:

```json
{
  "enabled": true,
  "url": "https://github.com/RochaProjects/AuraEssentials/releases/latest/download/kingdoms-glyph-pack-1.21.10.zip",
  "sha1": "707aaf51bfaeed6bbeee44e58fb66a176de8c439",
  "required": true,
  "prompt": "AuraEssentials",
  "additional_packs": [
    {
      "id": "aura_npc",
      "url": "https://gitlab.com/rochaproject/archive/-/raw/main/auraessentials/AuraNPC.zip?ref_type=heads",
      "sha1": "5ea0e0f8252d6c4f62097cc97bac447749b44032",
      "required": false,
      "prompt": "Pacote de modelos AuraNPC"
    }
  ]
}
```

O SHA-1 é o hash exigido pelo protocolo de pacote do Minecraft Java. Em servidor direto, o loader lê esse JSON e configura Kingdoms durante a fase de entrada, antes do jogador aparecer no mundo; AuraNPC entra como pacote adicional depois do spawn. Em backend Velocity com `forwarding.secret`, o mod envia os dois depois do spawn. Alterar URL, hash, obrigatoriedade ou mensagem não exige recompilar o engine; o jogador recebe a versão atualizada na próxima entrada.

## NPCs 3D Java

Os modelos AuraNPC importados ficam incorporados ao mod como definições pequenas de geometria; as texturas e os modelos de item vêm do pacote AuraNPC configurado acima. O servidor cria cada cubo como `item_display`, mais uma entidade `interaction` para o clique e um `text_display` opcional para o nome. Não cria jogador falso, skin nem armor stand. As ações configuradas rodam como o jogador que clicou, então comandos de jogador como `/kits` preservam as permissões e o contexto de quem interagiu. Use `{player}` ou `%player%` no texto da ação para inserir o nick; `{npc}` insere o nome visível.

- `/npc modelos` lista os modelos disponíveis; `/npc listar` mostra NPCs e `/npc info <id>` detalha um.
- `/npc criar <id> <modelo> <nome>` cria um NPC na sua dimensão, posição e orientação atuais.
- `/npc editar <id> nome|modelo|dimensao|pos|yaw|escala|hitbox|nomevisivel <valor>` altera os campos visuais e de localização; `pos` sem coordenadas usa sua posição atual.
- `/npc editar <id> comando <comando>` define a ação de clique; `comando adicionar|remover|limpar|listar` administra várias ações.
- `/npc remover <id>` apaga os dados e as entidades do modelo.

Os NPCs e ações ficam em `world/horizon-auraessentials/npcs.json`; recriar um NPC após trocar o modelo remove as partes antigas para não deixar cubos soltos.

## Geyser Standalone

O loader baixa a versão estável atual do Geyser Standalone da API oficial, valida o SHA-256 publicado pela API e mantém um único JAR em `.horizon/geyser/`. A cada inicialização, o loader inicia o Geyser junto do Horizon, encaminha o console do Geyser para os logs e encerra os dois processos juntos. A porta Bedrock padrão é `25606`; `GEYSER_PORT` pode sobrescrevê-la. A host precisa alocar e expor essa porta por TCP e UDP. A configuração gerada pelo Geyser fica em `.horizon/geyser/config.yml`.

## Base e licença

O sistema usa como base técnica o mod SMP nativo mantido em [`horizon-rust/horizon-mods`](https://github.com/horizon-rust/horizon-mods/tree/main/native/smp), sob MIT. O aviso de licença está em [LICENSE](LICENSE) e a documentação original em [README-UPSTREAM.md](README-UPSTREAM.md). O projeto Aura mantém os avisos e a licença da base.

## Compilação e instalação

Um `.hnmod` sozinho não instala recursos no servidor dedicado. O mod precisa ser incorporado a um engine Horizon e o servidor precisa iniciar usando esse engine. A compilação deve usar Linux x86_64, a mesma versão de Horizon do pacote de servidor e a ferramenta `native/tools/native.py` deste repositório.

O GitHub Actions gera o engine e as ferramentas do servidor, e publica o pacote em Releases deste repositório. O `server.jar` baixa esse pacote e prepara os recursos do Minecraft na primeira inicialização. Para evitar distribuir cópias extras dos JARs ou classes compiladas, o Horizon compila os exporters localmente durante essa preparação e apaga os arquivos temporários ao terminar.

## Mundos e dados de jogador

O `/mundo` usa dimensões `auraessentials:<nome>` em `world/dimensions/auraessentials/<nome>/`; não cria mundos paralelos na raiz. O engine gera uma dimensão nova com o gerador de Overworld quando ela não tem um stem próprio em datapack. Se um datapack registrar um tipo e um gerador específicos, o Horizon usa esses dados. A lista e os spawns ficam em `world/horizon-auraessentials/worlds.json`.

- `/mundo listar`, `criar <nome>`, `info <nome>`, `tp <nome>` e `spawn [nome]`;
- `/mundo setspawn`, `principal <nome|limpar>`, `carregar <nome>` e `descarregar <nome>`;
- `/mundo renomear <atual> <novo> confirmar`;
- `/mundo resetar <nome> confirmar [backup]` e `/mundo deletar <nome> confirmar [backup]`;
- `/mundo dados <estatisticas|conquistas|tudo> [jogador] [backup]` para você ou um jogador offline;
- `/mundo dados <estatisticas|conquistas|tudo> todos confirmar [backup]` para reset global;
- `/mundo gamerule <dimensão-customizada> <regra> [valor|global]` consulta, define ou remove um override.

Descarregar, resetar, renomear e apagar recusam dimensões com jogadores ou tickets ativos e salvam níveis carregados antes de alterar arquivos. O reset de dados afeta `players/stats` e `players/advancements`, incluindo o estado em memória do próprio administrador. Jogadores offline são encontrados pelo nome no `usercache.json` ou por UUID. O reset global exige confirmação e que os demais jogadores estejam desconectados; `backup` guarda cópias antes da alteração em `world/backups/auraessentials/`.

No Minecraft Java 26.3, `/gamerule` é global: o comando usa o `GameRuleMap` compartilhado e salvo em `data/minecraft/game_rules.dat`, então o mesmo valor vale no Overworld, Nether, End e dimensões customizadas. O Horizon mantém esse comportamento global. O `/mundo gamerule <dimensão> <regra> [valor|global]` deste mod adiciona overrides somente às dimensões customizadas: sem override, a dimensão acompanha a regra global; `global` remove o override. Os overrides são salvos em `world/data/horizon/dimension_game_rules.json`. Como o Horizon ainda compartilha o relógio e o clima, `advance_time` e `advance_weather` não aceitam override por dimensão. A implementação Java do AuraEssentials usa a API `World.getGameRuleValue`/`World.setGameRule` do Bukkit para o mundo informado.
