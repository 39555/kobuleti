# Kobuleti

## About 
Kobuleti is an rpg game about the village Kobuleti in Adjaria(Georgia) near Batumi where we lived in summer.

## Goals
The indent of this project is my initial expirience in Rust and async web apps development. While a build this game I ...

## Key Features
- Terminal based (with [ratatui])
- State machine actors built with pure [tokio]
- Login (simple, only by username)
- Reconnection
- Chat (saving between reconnections, while a game session is running)
- 2 players multiplayer together against monsters
- Turn-based

[ratatui]: https://docs.rs/ratatui/latest/ratatui/
[tokio]: https://tokio.rs


## Implementation
The client-server communication is simple. It works over tcp with serde::json and LinesCodec, which splits stream by '\0'. Client app wait an input or tcp event and then

### Actors with Tokio
The server of this game is split on different part: 'Peer' for 'accept connection' code and 'States' for common server part. The server does not use any Mutex or Rwlocks. Parts of the server based on 'Actor' pattern with only raw 'Tokio' as described in the [Alice Ryhl]'s article [Actors with Tokio]. The server only uses mpsc and oneshot channels to communicate between peer actors and the server actor.

[Actors with Tokio]: https://ryhl.io/blog/actors-with-tokio/


An actor splitted for an Actor and an actor Handle. An actor runs in own tokio task and listen on tokio::mpsc::channel receiver. Other parts of the server use handle for send enum messages, and receive return value through tokio::sync::oneshot::channel.

Also a handle of peer used for accept tcp connection messages and reduce it to its peer actor, or the server actor. It allows to avoid cycle  infinite awaiting leak.

For simplify defining actor api I make a macro with tt-muncher: 
```rust
actor_api! { // Peer::Intro
    impl  Handle<Msg<SharedCmd, IntroCmd>>{
        pub async fn set_username(&self, username: Username);
        pub async fn enter_game(&self, server: states::HomeHandle) -> Result<HomeHandle, RecvError>;
    }
}
```
The macro automatically defining inner enum and methods:
```rust
 enum IntroCmd {
    SetUsername(Username),
    EnterGame(tokio::sync::oneshot::Sender<HomeHandle>),
 }
impl Handle<Msg<SharedCmd, IntroCmd>> {
    pub async fn set_username(&self, username: Username){
        self.tx.send(Msg::State(IntroCmd::SetUsername(username)).expect("Open");
    }
    pub async fn enter_game(&self) -> Result<HomeHandle, RecvError>{
        let (tx, rx) = tokio::oneshot::channel();
        self.tx.send(Msg::State(IntroCmd::EnterGame(tx))).await.expect("Open");
        rx.await
    }
}
```



### State Machine
A game infrastructure is built on top of State Machine pattern to separate Login, Lobby, Character Creation and Game logic between server states.It gives a clean API interface, undestandable code, safe and scalability. Transition between states described simply (with article [State Machine] by [Ana Hoverbear]):
```rust
    // pseudo code for describe state machine transitions:

    let intro = Context::<Intro>::default();
    run(&mut intro).await;
    let home = Context::<Home>::from(intro);
    run(&mut home).await;
    let roles = Context::<Roles>::from(home);
    run(&mut roles).await;
    let game = Context::<Game>::from(roles);
    run(&mut game).await;

```

[State Machine]: https://hoverbear.org/blog/rust-state-machine-pattern/

The server does not validate messages, only parse different types of enum for each state.

States:

#### Intro
Intro is a login and handshake state. A client can login by username and enter to next server (new lobby or reconnect to game in Roles or Game states). On the client side after login Intro run tui and a start screen. If login failed, the app rejected with an error.

##### Sequence Diagram

```mermaid
sequenceDiagram
    actor C as Client::Intro
    box Peer
        participant PC as Peer::Intro(Tcp &<br/>PeerActorHandle)
        participant P as Peer::Intro (Actor)
    end
    box Server
        participant I as Server::Intro
        participant S as GameServer
    end
    C-)+PC: Login(Username)
    PC->>+I: LoginPlayer
    I-->I:  IsPlayerLimit
    I-->I:  IsUsernameExists
    opt If Some(server)
        I->>+S: GetPeerIdByName
        S-->>-I: Option
    
    opt Roles or Game (Reconnection)
        I->>+S:  IsConnected
        S-->>-I: bool<br/>(Login if player offline)
    end
    end

    I-)P: SetUsername<br/>(If Logged)

    I-->>-PC: Loggin Status

    break if login fails
        PC-->>C: AlreadyLogged,<br/> or PlayerLimit
    end


    PC--)-C: Logged
    PC->>+I: GetChatLog
    I->>+S: GetChatLog
    alt If Some(server)
        S-->>I: ChatLog
    else  None
        S-->>-I: EmptyChat
    end
    I-->>-PC: ChatLog
    PC-)C: ChatLog<br/>(ready to show)
    C-->C: Run Tui
    C -)+PC: EnterGame
    # end Intro
    PC->>-I: EnterGame
    alt server None
        I-)S: StartHome(Sender)
    else server Some(Home)
        I->>S: AddPeer(Sender)
    else server Some(Roles|Game)
        I->>+S: GetPeerHandle(Username)
        S-->>-I: OldPeerHandle
    participant PS as OldPeer::Offline<br/>(reconnection)
    I->>+P: Reconnect(Roles|Game)(<br/>ServerHandle, OldPeerHandle)
    P->>+PS: TakePeer
    PS-->>-P: Self
    # destroy Ps
    P--)-I: NewPeerHandle
    P-)C: Reconnect(StartData)
    I-)S: Reconnect(NewPeerHandle)
    S-->S: Peer Status Online
    end
    Note over C, S: Done Intro, Drop peer actor and handle, start new peer actor and handle.
    I-->I: Start Intro loop Again
    
    
```

#### Home
Home is a Lobby server. Here player waits other player. Players here already can use the chat. Home not allows Reconnection.

##### Sequence Diagram

```mermaid
sequenceDiagram
    actor C as Client::Home
    box Peer
        participant PC as Peer::Home(Tcp &<br/>PeerActorHandle)
        participant P as Peer::Home (Actor)
    end
    participant H as Server::Home
    
    C -)PC: StartRoles
    PC-)+H: StartRoles
    H-->H: Server if full?
    alt If Server is full, start Roles
    # cancel H
    H->H: Cancel
    participant R as Server::Roles
    H->>R: Start Roles::from(Home)
    loop Each peer (Force start for all)
        R->>+P: StartRoles(ServerHandle)
        P->P:  Cancel, Start Peer::Roles
        Note right of P: Now Peer::Roles
        P-->>-R: New PeerHandle
        P-)C: StartRoles
    end
    C-->R: Run Roles
    end

```

#### Roles
Roles is a state where a player should select own rpg role,and then start the game when all players ready. The Roles Allow reconnection if a player exit and enter with the same username.

##### Sequence Diagram

```mermaid
sequenceDiagram
    actor C as Client::Roles
    box Peer
        participant PC as Peer::Roles(Tcp &<br/> PeerActorHandle)
        participant P as Peer::Roles (Actor)
    end
    participant R as Server::Roles

    C -)+PC: SelectRole
    PC -)+R: SelectRole
    loop except sender,<br/> until Role==Role
        R->>+P: GetRole
        P->>-R: Role
        
    end
    alt if Role is available
        R->>+P: SetRole 
    end
    R-)PC: SendTcp(SelectedStatus)<br/>Busy, AlreadySelected
    PC-)-C: SelectedStatus
    loop Broadcast
        R-)P: SendTcp(AvailableRoles)
        P-)C: AvailableRoles
    end
    C-)+PC: StartGame
    PC-)R: StartGame
    R-->R: Are all have roles?
        alt If all have roles, start Game
    Note over PC, R: ... The same as in Home->Roles
    R->R: Cancel
    participant G as Server::Game
    R->>G: Start Game::from(Roles)
    loop Each peer (Force start for all)
        G->>+P: StartGame(ServerHandle)
        P->P:  Cancel, Start Peer::Game
        Note right of P: Now Peer::Game
        P-->>-G: New PeerHandle
        P-)+G: GetMonsters

    end
    C-->G: Ready Server::Game
    loop respond for async 'GetMonsters'
        G--)-P: MonstersForStart
        P-)C: StartGame(Data)
    end
    C-->+C: End Roles.<br/> Stop socket reader
    C-->-C: Start Game.<br/> Start socket reader
    par to active player
        G-)+P: SendTcp(Ready(DropAbility))
        P-)-C: Ready(DropAbility)
    and to other
        G-)+P: SendTcp(Wait)
        P-)-C: Wait
    end
    
    C-->G: Run Game
    end
```

#### Game
The Game state is a game session with 2 players. The Game allow reconnection. It manages in turn players turns.

##### Sequence Diagram


```mermaid
sequenceDiagram
    actor C as Client::Game
    box Peer
        participant PC as Peer::Game(Tcp &<br/> PeerActorHandle)
        participant P as Peer::Game (Actor)
    end
    participant G as Server::Game
    loop
        C-)+PC: DropAbility(ability)
        PC->>-P: DropAbility(ability)
        break ActivePlayer != Self
            PC-)C: TurnStatus::Err(Username)
        end
        P->>G: SwitchToNextPlayer
        G-->G: set next player<br/> and Phase
        par to active player
            G-)+P: Ready(SelectAbility))
            P-)-C: TurnStatus(Ready(SelectAbility))
        and to other
            G-)+P: Wait
            P-)-C: TurnStatus(Wait)
        end
        PC-)+G: BroadcastGameState
        loop expect sender
            G->>-P: SyncWithClient 
        end
        PC->>+P:  SyncWithClient
        P-)-C: UpdateGameData(Data)

        C-)+PC: SelectAbility(ability)
        PC->>-P: SelectAbility(ability)
        P->>G: SwitchToNextPlayer 
         Note over C, G: ... The same switch to next phase and player
        Note over C, G: ... TODO: Game over
    end
    
```

## Credits

- [Alice Ryhl]
- [Ana Hoverbear]
- [tokio and mini-redis project](https://github.com/tokio-rs/mini-redis)

[Alice Ryhl]: https://github.com/Darksonn/
[Ana Hoverbear]: https://github.com/hoverbear