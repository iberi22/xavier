#include <string.h>
#define LIMIT 8

static int pick(int mode, int len)
{
  if(mode == 1) {
    len = 0;
  }
#if defined(FEATURE_X)
  else if(mode == 2) {
    len = 1;
  }
#endif
  else {
    len = 2;
  }
#if 0
  never(len);
#else
  memset(&len, 0, LIMIT);
#endif
  return len;
}

int after(int v)
{
  return v + 1;
}
