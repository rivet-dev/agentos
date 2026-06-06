#include <netdb.h>
#ifdef gai_strerror
#undef gai_strerror
#endif
const char *(*foo)(int) = gai_strerror;
int main(void) { return 0; }
