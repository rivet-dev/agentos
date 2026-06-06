#include <netdb.h>
#ifdef getservbyname
#undef getservbyname
#endif
struct servent *(*foo)(const char *, const char *) = getservbyname;
int main(void) { return 0; }
