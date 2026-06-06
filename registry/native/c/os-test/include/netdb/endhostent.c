#include <netdb.h>
#ifdef endhostent
#undef endhostent
#endif
void (*foo)(void) = endhostent;
int main(void) { return 0; }
