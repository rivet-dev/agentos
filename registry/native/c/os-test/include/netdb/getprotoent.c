#include <netdb.h>
#ifdef getprotoent
#undef getprotoent
#endif
struct protoent *(*foo)(void) = getprotoent;
int main(void) { return 0; }
