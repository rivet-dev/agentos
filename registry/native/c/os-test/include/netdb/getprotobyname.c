#include <netdb.h>
#ifdef getprotobyname
#undef getprotobyname
#endif
struct protoent *(*foo)(const char *) = getprotobyname;
int main(void) { return 0; }
