#include <netdb.h>
#ifdef getservbyport
#undef getservbyport
#endif
struct servent *(*foo)(int, const char *) = getservbyport;
int main(void) { return 0; }
