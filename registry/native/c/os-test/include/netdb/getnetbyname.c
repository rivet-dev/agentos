#include <netdb.h>
#ifdef getnetbyname
#undef getnetbyname
#endif
struct netent *(*foo)(const char *) = getnetbyname;
int main(void) { return 0; }
