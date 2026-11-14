# Dependências do build do servidor

Este documento acompanha as decisões para reduzir o tempo do artefato dedicado (`horizon-engine`)
sem retirar código necessário ao servidor ou mudar o comportamento vanilla. Atualizar esta página
quando o grafo de dependências ou os comandos de release mudarem.

## Caminho de release atual

O workflow de release compila:

```sh
cargo build --locked --release -p horizon-client --no-default-features --features server-plugins --bin horizon-client
cargo build --locked --release -p horizon-server -p horizon-prepare
```

O primeiro executável é empacotado como `horizon-engine`. `horizon-server` supervisiona o processo
dedicado e `horizon-prepare` prepara os recursos de jogo na primeira instalação do loader. O plugin
Aura Loja não faz parte desse build do GitHub.

## Já removido do perfil de servidor

`--no-default-features` desliga `audio`, `controllers` e `horizon-extras`. Isso evita dependências
de áudio/controladores e não ativa recursos extras específicos do cliente. A feature
`server-plugins` habilita apenas o carregador e a API de plugins do servidor.

## Dependências ainda compartilhadas

O target dedicado é selecionado em runtime pelo executável `horizon-client`, mas gameplay e servidor
continuam no mesmo crate e compartilham módulos. Por isso, o perfil ainda compila partes do cliente,
incluindo `horizon-client-render`, `horizon-client-input`, `horizon-assets`, `wgpu`, `winit`,
`horizon-shaders`, `horizon-shader-api` e dependências de janela/gráficos. Embora o processo
dedicado não abra uma janela nem renderize, o código atual mantém referências compiladas a esses
módulos. Excluir essas dependências isoladamente não é um corte seguro.

## Decisão desta alteração

- Nenhum crate gráfico foi removido: o código compartilhado ainda depende dele ao compilar o
  executável dedicado.
- Nenhum artefato de cliente, launcher ou dado Mojang foi adicionado ao pacote do servidor.
- O workflow continua sem compilar o plugin Aura Loja.
- Corrigido o lint `collapsible_if` na migração do arquivo de saldo do plugin, que fazia o check CI
  falhar antes de validar os demais jobs.

## Próximo corte estrutural

Para deixar de compilar renderização no servidor, é preciso separar o target dedicado e suas
dependências do crate de gameplay/cliente, ou colocar módulos e dependências exclusivamente de
cliente atrás de uma feature/target que o binário dedicado não habilite. Esse corte deve ser medido
no workflow de release depois de implementado; removê-los agora do manifesto sem essa separação
quebraria referências do build.
