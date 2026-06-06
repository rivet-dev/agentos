#include <stdlib.h>
#ifdef secure_getenv
#undef secure_getenv
#endif
char *(*foo)(const char *) = secure_getenv;
int main(void) { return 0; }
