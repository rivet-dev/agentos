#include <netdb.h>
#ifdef setnetent
#undef setnetent
#endif
void (*foo)(int) = setnetent;
int main(void) { return 0; }
