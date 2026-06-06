#include <wchar.h>
#ifdef getwc
#undef getwc
#endif
wint_t (*foo)(FILE *) = getwc;
int main(void) { return 0; }
