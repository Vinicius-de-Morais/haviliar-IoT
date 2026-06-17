# Pontos que vou atualizar

1 - Adicionar os testes de campo que fizemos para medir a efetividade dos nossos dispositivos ESP (especificando os modelos)
2 - Buscar artigos que sustentem a efetividade de dispositivos ESP com alcance LoRa mais eficientes
3 - Inicialmente, planejavamos utilizar TTN, mas acredito que nao seja possivel pois é necessário que o Gateway tenha acesso a internet, nesse caso estarei voltando para a ideia dos gateways passarem a mensagem entre si, desse modo:
    -   Modo Normal: (Backend -> MQTT/Internet -> Gateway -> LAN -> Cancela )
    -   Modo Contingência: (Backend -> MQTT/Internet -> Gateway A (online) -> Gateway N (online) -> Gateway N+1 (offline) -> LAN -> Cancela )
        - Nesse caso, o Gateway N+1 retorna um feedback para o Gateway N enviar para o Servidor
4 - Por conta da nao utilizarmos mais a TTN, será necessário alterar todos os tópicos do artigo que mencionem ou sustentem a TTN, e adicionar uma linha na fundamentação mencionando a limitação da TTN e o motivo de não utilizarmos
5 - Estarei adicionando artigos que incentivem o uso do serializador CBOR em IoT na fundamentação teórica
