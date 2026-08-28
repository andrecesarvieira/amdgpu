# Revisão da interface — 28 de agosto de 2026

## Escopo

Revisão combinada de UX e acessibilidade da janela principal do AMDGPU Control,
considerando as duas GPUs detectadas no equipamento e a futura reimplementação
em Rust.

## Objetivo do usuário

Identificar rapidamente a GPU selecionada, compreender seu estado e ajustar
somente os controles realmente suportados pelo driver, sem confundir ausência de
capacidade com falha do aplicativo.

## Passos capturados

1. **GPU `0000:15:00.0`, visão inicial — Regular**
   - A GPU selecionada e a telemetria disponível ficam claras.
   - Três cartões sem dados e o perfil de carga desabilitado ocupam espaço sem
     oferecer uma ação.
   - Evidência: `01-current-igpu-top.png`.

2. **GPU `0000:03:00.0`, visão inicial — Boa**
   - Todos os sensores principais apresentam valores e os perfis disponíveis são
     compreensíveis.
   - O bloco de oito cartões domina a janela e empurra o objetivo principal,
     ajustar a GPU, para fora da primeira tela.
   - Evidência: `02-current-dgpu-top.png`.

3. **GPU `0000:03:00.0`, ajustes intermediários — Ruim**
   - Limite de potência aparece com faixa e valor padrão, o que transmite
     segurança.
   - Clock da GPU, clock da VRAM e voltagem são apresentados mesmo sendo
     impossíveis nesse hardware. O fragmento visual `a MHz` permanece na linha
     desabilitada e reduz a confiança na interface.
   - Evidência: `03-current-dgpu-middle.png`.

4. **GPU `0000:03:00.0`, final da página — Regular**
   - Inicialização automática e comportamento em segundo plano são explicados.
   - Zero RPM e curva da ventoinha continuam visíveis sem poder ser usados. O
     alerta sobre overclock e undervolt menciona recursos que a GPU selecionada
     não oferece.
   - Evidência: `04-current-dgpu-bottom.png`.

## Decisão de produto recomendada

- Não construir widgets para capacidades ausentes. A árvore GTK deve ser formada
  a partir de uma lista tipada de capacidades da GPU selecionada.
- Remover grupos inteiros quando nenhum controle daquele grupo estiver
  disponível.
- Diferenciar controles impossíveis no hardware de controles temporariamente
  inativos. Velocidade manual da ventoinha pode permanecer associada ao modo
  automático; Zero RPM ausente deve desaparecer completamente.
- Levar informações de diagnóstico sobre capacidades ausentes para uma tela
  secundária “Informações do hardware” ou para o relatório de diagnóstico.
- Tornar avisos contextuais: potência quando há limite de potência; OverDrive
  somente quando houver OverDrive.
- Compartilhar a GPU selecionada entre janela, persistência e system tray.
- Compactar a telemetria para que os ajustes suportados apareçam na primeira tela.

## Riscos de acessibilidade observáveis

- Textos desabilitados possuem contraste visual baixo e ainda recebem grande
  importância espacial.
- O estado indisponível depende principalmente de aparência acinzentada.
- Os alvos de clique aparentam ter tamanho adequado.
- Navegação por teclado, ordem de foco, nomes acessíveis e leitura por tecnologia
  assistiva não podem ser confirmados apenas pelas capturas.

## Referências geradas

- `concept-1.png`
- `concept-2.png`
- `concept-3.png`

As referências preservam GTK4/libadwaita, eliminam controles impossíveis e
exploram hierarquias diferentes para a futura versão Rust.
