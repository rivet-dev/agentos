#include <netdb.h>
#ifdef getservent
#undef getservent
#endif
struct servent *(*foo)(void) = getservent;
int main(void) { return 0; }
