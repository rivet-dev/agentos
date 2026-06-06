#include <netdb.h>
#ifdef endnetent
#undef endnetent
#endif
void (*foo)(void) = endnetent;
int main(void) { return 0; }
