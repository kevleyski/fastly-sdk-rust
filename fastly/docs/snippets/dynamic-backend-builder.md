Create a new dynamic backend builder.

The arguments are the name of the new backend to use, along with a string
describing the backend host. The latter can be of the form:

   * `"<ip address>"`
   * `"<hostname>"`
   * `"<ip address>:<port>"`
   * `"<hostname>:<port>"`

The name can be whatever you would like, as long as it does not match the name
of any of the static service backends nor match any other dynamic backends built
during this session. (Names can overlap between different sessions of the same
service -- they will be treated as completely separate entities and will not be
pooled -- but you cannot, for example, declare a dynamic backend named
"dynamic-backend" twice in the same session.)

The builder will start with default values for all other possible fields for the
backend, which can be overridden using the other methods provided. Call
`finish()` to complete the construction of the dynamic backend.

Dynamic backends must be enabled for this Compute@Edge service. You can determine
whether or not dynamic backends have been allowed for the current service by
using this builder, and then checking for the `BackendCreationError::Disallowed`
error result. This error only arises when attempting to use dynamic backends
with a service that has not had dynamic backends enabled, or dynamic backends
have been administratively prohibited for the node in response to an ongoing
incident.
