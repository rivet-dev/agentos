#include <netdb.h>
#ifdef getnetbyaddr
#undef getnetbyaddr
#endif
struct netent *(*foo)(uint32_t, int) = getnetbyaddr;
int main(void) { return 0; }
