#include <netdb.h>
#ifdef sethostent
#undef sethostent
#endif
void (*foo)(int) = sethostent;
int main(void) { return 0; }
