#include <netdb.h>
#ifdef getnetent
#undef getnetent
#endif
struct netent *(*foo)(void) = getnetent;
int main(void) { return 0; }
