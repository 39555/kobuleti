# Kobuleti

## State Machine

### Intro
Intro is a login and handshake state.

#### Sequence Diagram

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

### Home
Home is a Lobby server. 
#### Sequence Diagram

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
    Note left of H: If Server is full,<br/> start Roles
    # cancel H
    H->H: Cancel
    participant R as Server::Roles
    H->>R: Start Roles::from(Home)
    loop Each peer (Force start for all)
        R->>+P: StartRoles(ServerHandle)
        P->P:  Cancel, Start Peer::Roles
        Note left of H: Now Peer::Roles
        P-)C: StartRoles
        P-->>-R: New PeerHandle
    end
    R->R: Run Roles

```

### Roles

#### Sequence Diagram

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
        R-)PC: SendTcp(AvailableRoles)
        PC-)C: AvailableRoles
    end
    C-)+PC: StartGame
    PC-)R: StartGame
    R-->R: Are all have roles?
    Note left of R: If all have roles,<br/> start Game
    Note over PC, R: ... The same as in Home->Roles
    PC-)-C: StartGame(StartData)
```

### Game

#### Sequence Diagram
