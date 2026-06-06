#include <netdb.h>
#ifdef endprotoent
#undef endprotoent
#endif
void (*foo)(void) = endprotoent;
int main(void) { return 0; }
