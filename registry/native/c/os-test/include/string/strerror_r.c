#include <string.h>
#ifdef strerror_r
#undef strerror_r
#endif
int (*foo)(int, char *, size_t) = strerror_r;
int main(void) { return 0; }
