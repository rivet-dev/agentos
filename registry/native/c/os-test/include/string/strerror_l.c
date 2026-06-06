#include <string.h>
#ifdef strerror_l
#undef strerror_l
#endif
char *(*foo)(int, locale_t) = strerror_l;
int main(void) { return 0; }
