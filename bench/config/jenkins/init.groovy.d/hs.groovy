// HyperStack test bench: one user (HS_JENKINS_USER / HS_JENKINS_PASSWORD), an
// API token for it in /var/jenkins_home/hs-token, two executors, and the jobs
// from /run/hs/jobs/*.xml. Runs on every start and is safe to re-run.
import jenkins.model.Jenkins
import hudson.security.HudsonPrivateSecurityRealm
import hudson.security.FullControlOnceLoggedInAuthorizationStrategy
import jenkins.security.ApiTokenProperty

def env = System.getenv()
def id = env['HS_JENKINS_USER']
def pass = env['HS_JENKINS_PASSWORD']
if (!id || !pass) {
  throw new IllegalStateException('HS_JENKINS_USER and HS_JENKINS_PASSWORD must be set (bench/.env)')
}

def j = Jenkins.get()
def realm = j.getSecurityRealm()
if (!(realm instanceof HudsonPrivateSecurityRealm)) {
  realm = new HudsonPrivateSecurityRealm(false)
  j.setSecurityRealm(realm)
}
def user = realm.getUser(id)
if (user == null) {
  user = realm.createAccount(id, pass)
} else {
  user.addProperty(HudsonPrivateSecurityRealm.Details.fromPlainPassword(pass))
}
def strategy = new FullControlOnceLoggedInAuthorizationStrategy()
strategy.setAllowAnonymousRead(false)
j.setAuthorizationStrategy(strategy)
j.setNumExecutors(2)
j.save()

def tokenFile = new File(j.getRootDir(), 'hs-token')
if (!tokenFile.exists()) {
  def t = user.getProperty(ApiTokenProperty).tokenStore.generateNewToken('hyperstack')
  user.save()
  tokenFile.text = t.plainValue
}

def jobs = new File('/run/hs/jobs')
if (jobs.isDirectory()) {
  jobs.listFiles().findAll { it.name.endsWith('.xml') }.each { f ->
    def name = f.name - '.xml'
    if (j.getItem(name) == null) {
      f.withInputStream { j.createProjectFromXML(name, it) }
    }
  }
}
