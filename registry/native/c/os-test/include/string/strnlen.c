#include <string.h>
#ifdef strnlen
#undef strnlen
#endif
size_t (*foo)(const char *, size_t) = strnlen;
int main(void) { return 0; }
