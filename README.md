# Auto-batching proxy for inference requests

## Task Description

The task was to create an auto-batching proxy service that would serve as a wrapper
over another inference service (see `Makefile` for details on how we are launching that
service for development purposes). Internally, that batching proxy will 🥁 batch
individual embedding requests, while for the end-user the API is the same, as if
they were the only client of the inference service. This batching makes requests
to the upstream service more efficient (and helps reduce costs).

In my hometown in 1990-2000s, there used to be drivers hanging around the realway
station who - if you missed your train or just did not bother to buy a ticket, would
offer you a ride to another town - but a shared ride. They would gather (batch)
a few fellas like myself and then start the ride. But there were rules - they
could not take more that N people (depending on the vehicle size) and one person
who came first could not wait for too long (like no longer than an hour normally).

In the similar fashion, in our batching service here we got `MAX_BATCH_SIZE` and
`MAX_WAIT_TIME` (in millis) parameters configurable via the environment (see other configurable
options in `.env.example`).

## Solution

### Stack

Our REST API wrapper is powered by axum web-framework, which is our framework
of choice. We have not added Openapi definitions to this project, but if we need
to, we will integrate `utoipa` crate. All other crates we depend on are pretty
standard.

### Interface

Our REST API wrapper is powered by axum and currently provides one single
endpoint `POST /embed` and so trying it our is super straightforward:

```console
curl 127.0.0.1:8081/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

Notice how it looks exactly the same (but for PORT number) if you were querying
the [upstream service][4] directly:

```console
curl 127.0.0.1:8080/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

### Handler

On the app's start-up, we are launching a task with the web-server and a dedicated
task for our inference service worker - which is effectively an actor responsible
for inference and hiding the implementation details from the axum handler to separate
concerns. Instances of the handler (threads processing end-users' requests) are
communicating with that worker using channels and messages. Once a handler receives
a request it forwards it to the worker and awaits the worker's response (embeddings or
an error) via a oneshot channel, and once it gets the response, it sends it's
JSON representation to the end-user.

### Worker

The worker just listens for messages from the axum handlers. The worker keeps
some state: it has got a message queue with a capacity as per `MAX_BATCH_SIZE`
and a timeout as per `MAX_WAIT_TIME` - whichever comes first will make the worker
to send the batch to the upstream service. If an error is received from the
upstream inference service, it gets "broadcast" to the handlers. If the embeddings
are received, the worker will make sure to not send the entirety of it to each
handler, rather only the segment that corresponds to the handler's inputs. We
are relying here on the fact that the upstream service returns an array of embeddings
in which an embdedding at index N is the result for the query at index N in the
inputs container in our request.

To give a concrete example, image the batch size is set to `2`, and the first
request contains inputs array `["hello", "world"]` while the second request has
only one item `["bye"]` - the worker will flatten these two into one array and
send to the upstream service as `["hello", "world", "bye"]`. The response our worker
gets will have the following shape:
`[[-0.045, ... , -0.123144], [0.412, ..., -0.412], [0.1241, ..., 0.123]]`.
The worker still "remembers" at that point that it needs to send `2` embeddings
to the first handler and `1` embeddig to the second handler instance.

Also - replaying the example above - if the batch size is set to `2`, the worker
received a message from one handler `["hello", "world"]` and the time-out
(configured via `MAX_WAIT_TIME`) is reached, the worker will send
send `["hello", "world"]` to the upstream server.

Each time the batch get "flushed", the timeout gets unset and the queue gets
emptied.

## Demo

NB: make sure you got [`GNU Make`][2], and [`docker`][3] installed.

Populate your very own local `.env` file with:

```console
make dotenv
```

You can now launch the auto-batching proxy together with the inference service
with a single command:

```console
docker compose up --build
```

The command above will build our proxy app, launch the upstream inference service
first, make sure it is ready, and then launch the proxy app.

If you tweak `MAX_WAIT_TIME` and `MAX_BATCH_SIZE` parameters in your `.env`
file, make sure to restart the containers.

## Dev Setup

Make sure you got [`cargo`][1], [`GNU Make`][2], and [`docker`][3] installed,
and hit:

```console
make setup
```

You should now be able to start the back-end in watch mode with:

```console
make watch
```

You can send requests with:

```console
curl 127.0.0.1:8081/embed -X POST -d '{"inputs":["What is Vector Search?", "Hello, world!"]}' -H 'Content-Type: application/json'
```

You can also tweak configurations in the generated `.env` file (gets populated
via `make setup`), the dev-server will restart automatically (if you are using
the `make watch` command as described above).

<!-- -------------------------------- LINKS -------------------------------- -->
[1]: https://doc.rust-lang.org/cargo/getting-started/installation.html
[2]: https://www.gnu.org/software/make/
[3]: https://docs.docker.com/engine/install/
[4]: https://github.com/huggingface/text-embeddings-inference
