#include <netdb.h>
#ifdef setprotoent
#undef setprotoent
#endif
void (*foo)(int) = setprotoent;
int main(void) { return 0; }
