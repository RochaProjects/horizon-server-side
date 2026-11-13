# AuraEssentials para Horizon

Mod nativo de servidor em Rust, feito para o Horizon 26.3. O projeto começa com o sistema de locais pessoais e foi organizado para crescer para teleportes, terrenos, lobby protegido, loja e administração.

## Comandos de locais

- `/sethome <nome>` salva a posição atual com esse nome. Repetir o comando com o mesmo nome move o local.
- `/home <nome>` inicia o teleporte para o local salvo.
- `/home` e `/homes` mostram a lista de locais do jogador.
- Ao completar `/home `, o servidor sugere os nomes salvos pelo próprio jogador.
- `/delhome <nome>` remove um local.

Os nomes usam até 16 letras, números, `_` ou `-`. Os locais ficam associados ao UUID e incluem dimensão, coordenadas e orientação. O limite inicial vem da configuração do mod e pode crescer com o tempo jogado.

## Base e licença

O sistema usa como base técnica o mod SMP nativo mantido em [`horizon-rust/horizon-mods`](https://github.com/horizon-rust/horizon-mods/tree/main/native/smp), sob MIT. O aviso de licença está em [LICENSE](LICENSE) e a documentação original em [README-UPSTREAM.md](README-UPSTREAM.md). O projeto Aura mantém os avisos e a licença da base.

## Compilação e instalação

Um `.hnmod` sozinho não instala recursos no servidor dedicado. O mod precisa ser incorporado a um engine Horizon e o servidor precisa iniciar usando esse engine. A compilação deve usar Linux x86_64, a mesma versão de Horizon do pacote de servidor e a ferramenta `native/tools/native.py` deste repositório.

O fluxo de publicação planejado é gerar um pacote do engine no GitHub Actions, publicá-lo em Releases deste repositório e fazer o `server.jar` baixar o pacote compatível. O loader ainda usa o engine padrão até essa integração ser publicada.

## Próximos módulos

1. Completar e validar os locais em jogo.
2. Traduzir e adaptar TPA, `/back`, `/spawn` e warps.
3. Adaptar terrenos, permissões, confiança e proteção de lobby.
4. Adaptar loja e menus de baú.
5. Planejar gerenciamento de mundos pelo supervisor do servidor. Criar, renomear e apagar mundos exige coordenação com o processo Horizon e não será feito apagando pastas enquanto um mundo estiver em uso.
