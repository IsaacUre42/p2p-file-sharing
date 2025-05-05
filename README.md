# Libp2p Rust Filesharing and Chat

To run the rendezvous_server: cargo run --bin rendezvous-server \
To run the client: cargo run --bin file-sharing 

### Client Commands:
1. **reg <username\> :** \
Recommended first command, your messages will only be discoverable with a username
2. **msg "<message\>" :** \
Send a message to the open chat room. The initial chat room is called 'default'. \
NOTE! Your first message will attempt to load your registered name to the network. Send a message before attempting to dm.
3. **topic <topic\> :** \
Change chat room to the one specified. Not recommended to use as default behaviour should be fine.
4. **dial <username\>:** \
Directly connect to the peer by username to be able to 'dm' and fileshare. Command may be redeundant.
5. **dm <username\> <message\>:** \
Attempt to send a private message to 'username'. Will only work with a direct connection (e.g. dialed), 
peers on the same local network should be connected anyways.
6. **offer <filename\> :**\
Makes a file visible to peers on the network. The filename is accessed from the directory where the client was run.
7. **get <username\>:** \
Gets all the files that that user has made visible.
8. **trade <username\> <your_file> <their_file>**: \
Makes a trade offer, your file for their file. Both files must be currently visible on the network. \
The peer will be asked to 'accept' or 'deny' the trade offer.
9. **discovery <address\>:** \
Manually connect to the rendezvous server.

### Example Usage
_(assuming existence of files test1.txt and test2.txt)_

Client 1: cargo run --bin file-sharing \
Client 2: cargo run --bin file-sharing \
Client 1: reg john \
Client 2: reg jane \
Client 1: msg hi \
Client 2: msg hey \
Client 1: dm jane "want to trade?" \
Client 1: offer test1.txt \
Client 2: offer test2.txt \
Client 1: get jane // _just for illustration, should output 'test2.txt'_ \
Client 2: trade john test2.txt test1.txt \
Client 1: accept // _files should be transferred into 'received' folder._

### Disclaimers
The server code for the rendezvous server is mostly from the libp2p git repository. \
Discovery using the server hasn't been thoroughly tested but has worked in isolated environments.\
A lot of AI assistance was utilized on the server discovery code, if the error handling etc looks a bit different 
(probably more thorough) then that is probably why. \
The code is quite fragile with minimal error handling. The above example should hopefully demonstrate the \
full implemented functionality, but most messing around will probably break things.


