#include <netdb.h>
#ifdef endservent
#undef endservent
#endif
void (*foo)(void) = endservent;
int main(void) { return 0; }
