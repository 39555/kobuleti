# kobuleti

## About 
`kobuleti` is an RPG game that revolves around playing cards. The game story is about the village of `Kobuleti`.  It draws inspiration from `Ascension` by game designer [Antony Ngo] which uses playing cards.

[Antony Ngo]: https://www.youtube.com/watch?v=NwLkOBRf1iM&ab_channel=AnthonyNgo

## Goals
The intent of this project was to initiate my experience in Rust and asynchronous web app development by creating this very personal game about a small Georgian village, where I was lucky to spend a summer. My girlfriend, Xenia, who worked on this game as an illustrator, and I wanted to share with the world a piece of this mysterious place named (სოფელი, '_sopeli_' in Georgian) `Kobuleti`.

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

## Gallery


## Implementation
The client-server communication is simple. It operates over TCP with *serde::json* and *tokio_util::codec::LinesCodec*, which divides a TCP stream by '_\0_'. A client app awaits for input or a TCP event before rendering a new state.

### Actors with Tokio
The server for this game is divided into different parts, each implemented as a tokio task: 
- `Peer` handles "_accept connection_" code. Task spawns for each connection.
- `States` manages the common server functionality and includes two tokio tasks: the Intro server and the game server.

The server does not use any Mutex or Rwlocks. Instead, it relies on the `Actor` pattern, as described in [Alice Ryhl]'s article [Actors with Tokio]. Communication between peer actors and the server actor is accomplished using _mpsc_ and _oneshot_ channels

[Actors with Tokio]: https://ryhl.io/blog/actors-with-tokio/

An actor is split into two parts: an *Actor* and an actor *Handle*. The Actor runs in own tokio task and listens on _tokio::mpsc::channel_ receiver. Other parts of the server use Handle to send enum messages and receive return value through _tokio::sync::oneshot::channel_.

The Peer handle is also used for accepting TCP connection messages and directing them to the appropriate peer actor or the server actor. This approach helps prevent potential infinite awaiting leaks.

To simplify the definition of the actor API, I've created a macro with an incremental tt-muncher.


```rust
actor_api! { // Peer::Intro
    impl  Handle<Msg<SharedCmd, IntroCmd>>{
        pub async fn set_username(&self, username: Username);
        pub async fn enter_game(&self, server: states::HomeHandle) -> Result<HomeHandle, RecvError>;
    }
}
```
The macro automatically defines inner enum and methods:
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
The game infrastructure is constructed using the State Machine pattern, which effectively segregates the logic for "Login," "Lobby," "Character Creation," and "Game" into distinct server states. It gives a clean API interface, undestandable code, safe and scalability. The transitions between states are described in article [State Machine Pattern] by [Ana Hoverbear]:


```rust
    // a pseudo code for describe state machine transitions:

    let intro = Context::<Intro>::default();
    run(&mut intro).await;
    let home = Context::<Home>::from(intro);
    run(&mut home).await;
    let roles = Context::<Roles>::from(home);
    run(&mut roles).await;
    let game = Context::<Game>::from(roles);
    run(&mut game).await;

```

[State Machine Pattern]: https://hoverbear.org/blog/rust-state-machine-pattern/

The server does not validate messages, it only parses enum types for each state. Now let's look into each states:

#### Intro
The `Intro` is a login state. In this state, clients can log in using their username and proceed to the next server (either a new lobby or a reconnection to a game in "Roles" or "Game" states). On the client side, after a successful login, the "Intro" state initiates a TUI and displays a start screen. If the login attempt fails, the application is rejected with an error.

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
The "Home" state functions as a lobby server where players wait for other players and chat. Reconnection is not allowed in this state.

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
In the "Roles" state, players choose their RPG roles and start the game once all players are ready. This state allows reconnection if a player exits and reenters with the same username.

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
The "Game" state represents a game session with two players and allows reconnection. It manages player turns in a sequential manner.

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