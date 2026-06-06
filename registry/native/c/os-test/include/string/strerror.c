#include <string.h>
#ifdef strerror
#undef strerror
#endif
char *(*foo)(int) = strerror;
int main(void) { return 0; }
