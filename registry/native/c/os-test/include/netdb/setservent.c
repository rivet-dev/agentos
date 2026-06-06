#include <netdb.h>
#ifdef setservent
#undef setservent
#endif
void (*foo)(int) = setservent;
int main(void) { return 0; }
